# This file provide a Rust overlay, which provides pre-packaged bleeding edge versions of rustc
# and cargo.
self: super:

let
  fromTOML =
    # nix 2.1 added the fromTOML builtin
    if builtins ? fromTOML
    then builtins.fromTOML
    else (import ./parseTOML.nix).fromTOML;

  parseRustToolchain = file: with builtins;
    if file == null then
      {}
    else
    let
      # matches toolchain descriptions of type "nightly" or "nightly-2020-01-01"
      channel_by_name = match "([a-z]+)(-([0-9]{4}-[0-9]{2}-[0-9]{2}))?.*" (readFile file);
      # matches toolchain descriptions of type "1.34.0" or "1.34.0-2019-04-10"
      channel_by_version = match "([0-9]+\\.[0-9]+\\.[0-9]+)(-([0-9]{4}-[0-9]{2}-[0-9]{2}))?.*" (readFile file);
    in
      (x: { channel = head x; date = (head (tail (tail x))); }) (
        if channel_by_name != null then
          channel_by_name
        else
          channel_by_version
        );

  # See https://github.com/rust-lang-nursery/rustup.rs/blob/master/src/dist/src/dist.rs
  defaultDistRoot = "https://static.rust-lang.org";
  manifest_v1_url = {
    dist_root ? defaultDistRoot + "/dist",
    date ? null,
    staging ? false,
    # A channel can be "nightly", "beta", "stable", or "\d{1}\.\d{1,3}\.\d{1,2}".
    channel ? "nightly",
    # A path that points to a rust-toolchain file, typically ./rust-toolchain.
    rustToolchain ? null,
    ...
  }:
    let args = { inherit channel date; } // parseRustToolchain rustToolchain; in
    let inherit (args) date channel; in
    if date == null && staging == false
    then "${dist_root}/channel-rust-${channel}"
    else if date != null && staging == false
    then "${dist_root}/${date}/channel-rust-${channel}"
    else if date == null && staging == true
    then "${dist_root}/staging/channel-rust-${channel}"
    else throw "not a real-world case";

  manifest_v2_url = args: (manifest_v1_url args) + ".toml";

  getComponentsWithFixedPlatform = pkgs: pkgname: stdenv:
    let
      pkg = pkgs.${pkgname};
      srcInfo = pkg.target.${super.rust.toRustTarget stdenv.targetPlatform} or pkg.target."*";
      components = srcInfo.components or [];
      componentNamesList =
        builtins.map (pkg: pkg.pkg) (builtins.filter (pkg: (pkg.target != "*")) components);
    in
      componentNamesList;

  getExtensions = pkgs: pkgname: stdenv:
    let
      inherit (super.lib) unique;
      pkg = pkgs.${pkgname};
      srcInfo = pkg.target.${super.rust.toRustTarget stdenv.targetPlatform} or pkg.target."*";
      extensions = srcInfo.extensions or [];
      extensionNamesList = unique (builtins.map (pkg: pkg.pkg) extensions);
    in
      extensionNamesList;

  hasTarget = pkgs: pkgname: target:
    pkgs ? ${pkgname}.target.${target};

  getTuples = pkgs: name: targets:
    builtins.map (target: { inherit name target; }) (builtins.filter (target: hasTarget pkgs name target) targets);

  # In the manifest, a package might have different components which are bundled with it, as opposed as the extensions which can be added.
  # By default, a package will include the components for the same architecture, and offers them as extensions for other architectures.
  #
  # This functions returns a list of { name, target } attribute sets, which includes the current system package, and all its components for the selected targets.
  # The list contains the package for the pkgTargets as well as the packages for components for all compTargets
  getTargetPkgTuples = pkgs: pkgname: pkgTargets: compTargets: stdenv:
    let
      inherit (builtins) elem;
      inherit (super.lib) intersectLists;
      components = getComponentsWithFixedPlatform pkgs pkgname stdenv;
      extensions = getExtensions pkgs pkgname stdenv;
      compExtIntersect = intersectLists components extensions;
      tuples = (getTuples pkgs pkgname pkgTargets) ++ (builtins.map (name: getTuples pkgs name compTargets) compExtIntersect);
    in
      tuples;

  getFetchUrl = pkgs: pkgname: target: stdenv: fetchurl:
    let
      pkg = pkgs.${pkgname};
      srcInfo = pkg.target.${target};
    in
      (super.fetchurl { url = srcInfo.xz_url; sha256 = srcInfo.xz_hash; });

  checkMissingExtensions = pkgs: pkgname: stdenv: extensions:
    let
      inherit (builtins) head;
      inherit (super.lib) concatStringsSep subtractLists;
      availableExtensions = getExtensions pkgs pkgname stdenv;
      missingExtensions = subtractLists availableExtensions extensions;
      extensionsToInstall =
        if missingExtensions == [] then extensions else throw ''
          While compiling ${pkgname}: the extension ${head missingExtensions} is not available.
          Select extensions from the following list:
          ${concatStringsSep "\n" availableExtensions}'';
    in
      extensionsToInstall;

  getComponents = pkgs: pkgname: targets: extensions: targetExtensions: stdenv: fetchurl:
    let
      inherit (builtins) head map;
      inherit (super.lib) flatten remove subtractLists unique;
      targetExtensionsToInstall = checkMissingExtensions pkgs pkgname stdenv targetExtensions;
      extensionsToInstall = checkMissingExtensions pkgs pkgname stdenv extensions;
      hostTargets = [ "*" (super.rust.toRustTarget stdenv.hostPlatform) (super.rust.toRustTarget stdenv.targetPlatform) ];
      pkgTuples = flatten (getTargetPkgTuples pkgs pkgname hostTargets targets stdenv);
      extensionTuples = flatten (map (name: getTargetPkgTuples pkgs name hostTargets targets stdenv) extensionsToInstall);
      targetExtensionTuples = flatten (map (name: getTargetPkgTuples pkgs name targets targets stdenv) targetExtensionsToInstall);
      pkgsTuples = pkgTuples ++ extensionTuples ++ targetExtensionTuples;
      missingTargets = subtractLists (map (tuple: tuple.target) pkgsTuples) (remove "*" targets);
      pkgsTuplesToInstall =
        if missingTargets == [] then pkgsTuples else throw ''
          While compiling ${pkgname}: the target ${head missingTargets} is not available for any package.'';
    in
      map (tuple: { name = tuple.name; src = (getFetchUrl pkgs tuple.name tuple.target stdenv fetchurl); }) pkgsTuplesToInstall;

  installComponents = stdenv: namesAndSrcs:
    let
      inherit (builtins) map;
      installComponent = name: src:
        stdenv.mkDerivation {
          inherit name;
          inherit src;

          # No point copying src to a build server, then copying back the
          # entire unpacked contents after just a little twiddling.
          preferLocalBuild = true;

          # (@nbp) TODO: Check on Windows and Mac.
          # This code is inspired by patchelf/setup-hook.sh to iterate over all binaries.
          installPhase = ''
            patchShebangs install.sh
            CFG_DISABLE_LDCONFIG=1 ./install.sh --prefix=$out --verbose

            setInterpreter() {
              local dir="$1"
              [ -e "$dir" ] || return 0

              header "Patching interpreter of ELF executables and libraries in $dir"
              local i
              while IFS= read -r -d ''$'\0' i; do
                if [[ "$i" =~ .build-id ]]; then continue; fi
                if ! isELF "$i"; then continue; fi
                echo "setting interpreter of $i"
                
                if [[ -x "$i" ]]; then
                  # Handle executables
                  patchelf \
                    --set-interpreter "$(cat $NIX_CC/nix-support/dynamic-linker)" \
                    --set-rpath "${super.lib.makeLibraryPath [ self.zlib ]}:$out/lib" \
                    "$i" || true
                else
                  # Handle libraries
                  patchelf \
                    --set-rpath "${super.lib.makeLibraryPath [ self.zlib ]}:$out/lib" \
                    "$i" || true
                fi
              done < <(find "$dir" -type f -print0)
            }

            setInterpreter $out
          '';

          postFixup = ''
            # Function moves well-known files from etc/
            handleEtc() {
              local oldIFS="$IFS"

              # Directories we are aware of, given as substitution lists
              for paths in \
                "etc/bash_completion.d","share/bash_completion/completions","etc/bash_completions.d","share/bash_completions/completions";
                do
                # Some directoties may be missing in some versions. If so we just skip them.
                # See https://github.com/mozilla/nixpkgs-mozilla/issues/48 for more infomation.
                if [ ! -e $paths ]; then continue; fi

                IFS=","
                set -- $paths
                IFS="$oldIFS"

                local orig_path="$1"
                local wanted_path="$2"

                # Rename the files
                if [ -d ./"$orig_path" ]; then
                  mkdir -p "$(dirname ./"$wanted_path")"
                fi
                mv -v ./"$orig_path" ./"$wanted_path"

                # Fail explicitly if etc is not empty so we can add it to the list and/or report it upstream
                rmdir ./etc || {
                  echo Installer tries to install to /etc:
                  find ./etc
                  exit 1
                }
              done
            }

            if [ -d "$out"/etc ]; then
              pushd "$out"
              handleEtc
              popd
            fi
          '';

          dontStrip = true;
        };
    in
      map (nameAndSrc: (installComponent nameAndSrc.name nameAndSrc.src)) namesAndSrcs;

  # Manifest files are organized as follow:
  # { date = "2017-03-03";
  #   pkg.cargo.version= "0.18.0-nightly (5db6d64 2017-03-03)";
  #   pkg.cargo.target.x86_64-unknown-linux-gnu = {
  #     available = true;
  #     hash = "abce..."; # sha256
  #     url = "https://static.rust-lang.org/dist/....tar.gz";
  #     xz_hash = "abce..."; # sha256
  #     xz_url = "https://static.rust-lang.org/dist/....tar.xz";
  #   };
  # }
  #
  # The packages available usually are:
  #   cargo, rust-analysis, rust-docs, rust-src, rust-std, rustc, and
  #   rust, which aggregates them in one package.
  #
  # For each package the following options are available:
  #   extensions        - The extensions that should be installed for the package.
  #                       For example, install the package rust and add the extension rust-src.
  #   targets           - The package will always be installed for the host system, but with this option
  #                       extra targets can be specified, e.g. "mips-unknown-linux-musl". The target
  #                       will only apply to components of the package that support being installed for
  #                       a different architecture. For example, the rust package will install rust-std
  #                       for the host system and the targets.
  #   targetExtensions  - If you want to force extensions to be installed for the given targets, this is your option.
  #                       All extensions in this list will be installed for the target architectures.
  #                       *Attention* If you want to install an extension like rust-src, that has no fixed architecture (arch *),
  #                       you will need to specify this extension in the extensions options or it will not be installed!
  fromManifestFile = manifest: { stdenv, fetchurl, patchelf }:
    let
      inherit (builtins) elemAt;
      inherit (super) makeOverridable;
      inherit (super.lib) flip mapAttrs;
      pkgs = fromTOML (builtins.readFile manifest);
    in
    flip mapAttrs pkgs.pkg (name: pkg:
      makeOverridable ({extensions, targets, targetExtensions}:
        let
          version' = builtins.match "([^ ]*) [(]([^ ]*) ([^ ]*)[)]" pkg.version;
          version = "${elemAt version' 0}-${elemAt version' 2}-${elemAt version' 1}";
          namesAndSrcs = getComponents pkgs.pkg name targets extensions targetExtensions stdenv fetchurl;
          components = installComponents stdenv namesAndSrcs;
          componentsOuts = builtins.map (comp: (super.lib.strings.escapeNixString (super.lib.getOutput "out" comp))) components;
        in
          super.pkgs.symlinkJoin {
            name = name + "-" + version;
            paths = components;
            postBuild = ''
              # If rustc or rustdoc is in the derivation, we need to copy their
              # executable into the final derivation. This is required
              # for making them find the correct SYSROOT.
              # Similarly, we copy the python files for gdb pretty-printers since
              # its auto-load-safe-path mechanism doesn't like symlinked files.
              for target in $out/bin/{rustc,rustdoc} $out/lib/rustlib/etc/*.py; do
                if [ -e $target ]; then
                  cp --remove-destination "$(realpath -e $target)" $target
                fi
              done
            '';

            # Add the compiler as part of the propagated build inputs in order
            # to run:
            #
            #    $ nix-shell -p rustChannels.stable.rust
            #
            # And get a fully working Rust compiler, with the stdenv linker.
            propagatedBuildInputs = [ stdenv.cc ];

            meta.platforms = super.lib.platforms.all;
          }
      ) { extensions = []; targets = []; targetExtensions = []; }
    );

  fromManifest = sha256: manifest: { stdenv, fetchurl, patchelf }:
    let manifestFile = if sha256 == null then builtins.fetchurl manifest else fetchurl { url = manifest; inherit sha256; };
    in fromManifestFile manifestFile { inherit stdenv fetchurl patchelf; };

in

rec {
  lib = super.lib // {
    inherit fromTOML;
    rustLib = {
      inherit fromManifest fromManifestFile manifest_v2_url;
    };
  };

  rustChannelOf = { sha256 ? null, ... } @ manifest_args: fromManifest
    sha256 (manifest_v2_url manifest_args)
    { inherit (self) stdenv fetchurl patchelf; }
    ;

  # Set of packages which are automagically updated. Do not rely on these for
  # reproducible builds.
  latest = (super.latest or {}) // {
    rustChannels = {
      nightly = rustChannelOf { channel = "nightly"; };
      beta    = rustChannelOf { channel = "beta"; };
      stable  = rustChannelOf { channel = "stable"; };
    };
  };

  # Helper builder
  rustChannelOfTargets = channel: date: targets:
    (rustChannelOf { inherit channel date; })
      .rust.override { inherit targets; };

  # For backward compatibility
  rustChannels = latest.rustChannels;

  # For each channel:
  #   latest.rustChannels.nightly.cargo
  #   latest.rustChannels.nightly.rust   # Aggregate all others. (recommended)
  #   latest.rustChannels.nightly.rustc
  #   latest.rustChannels.nightly.rust-analysis
  #   latest.rustChannels.nightly.rust-docs
  #   latest.rustChannels.nightly.rust-src
  #   latest.rustChannels.nightly.rust-std

  # For a specific date:
  #   (rustChannelOf { date = "2017-06-06"; channel = "beta"; }).rust
}
