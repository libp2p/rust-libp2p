with builtins;

# Tokenizer.
let
  layout_pat = "[ \n]+";
  layout_pat_opt = "[ \n]*";
  token_pat = ''=|[[][[][a-zA-Z0-9_."*-]+[]][]]|[[][a-zA-Z0-9_."*-]+[]]|[[][^]]+[]]|[a-zA-Z0-9_-]+|"[^"]*"''; #"

  tokenizer_1_11 = str:
    let
      tokenizer_rec = len: prevTokens: patterns: str:
        let
          pattern = head patterns;
          layoutAndTokens = match pattern str;
          matchLength = stringLength (head layoutAndTokens);
          tokens = prevTokens ++ tail layoutAndTokens;
        in
          if layoutAndTokens == null then
            # if we cannot reduce the pattern, return the list of token
            if tail patterns == [] then prevTokens
            # otherwise, take the next pattern, which only captures half the token.
            else tokenizer_rec len prevTokens (tail patterns) str
          else tokenizer_rec len tokens patterns (substring matchLength len str);

      avgTokenSize = 100;
      ceilLog2 = v:
        let inner = n: i: if i < v then inner (n + 1) (i * 2) else n; in
        inner 1 1;

      # The builtins.match function match the entire string, and generate a list of all captured
      # elements. This is the most efficient way to make a tokenizer, if we can make a pattern which
      # capture all token of the file. Unfortunately C++ std::regex does not support captures in
      # repeated patterns. As a work-around, we generate patterns which are matching tokens in multiple
      # of 2, such that we can avoid iterating too many times over the content.
      generatePatterns = str:
        let
          depth = ceilLog2 (stringLength str / avgTokenSize);
          inner = depth:
            if depth == 0 then [ "(${token_pat})" ]
            else
              let next = inner (depth - 1); in
              [ "${head next}${layout_pat}${head next}" ] ++ next;
        in
          map (pat: "(${layout_pat_opt}${pat}).*" ) (inner depth);

    in
      tokenizer_rec (stringLength str) [] (generatePatterns str) str;

  tokenizer_1_12 = str:
    let
      # Nix 1.12 has the builtins.split function which allow to tokenize the
      # file quickly. by iterating with a simple regexp.
      layoutTokenList = split "(${token_pat})" str;
      isLayout = s: match layout_pat_opt s != null;
      filterLayout = list:
        filter (s:
          if isString s then
            if isLayout s then false
            else throw "Error: Unexpected token: '${s}'"
          else true) list;
      removeTokenWrapper = list:
        map (x: assert tail x == []; head x) list;
    in
      removeTokenWrapper (filterLayout layoutTokenList);

  tokenizer =
    if builtins ? split
    then tokenizer_1_12
    else tokenizer_1_11;
in

# Parse entry headers
let
  unescapeString = str:
    # Let's ignore any escape character for the moment.
    assert match ''"[^"]*"'' str != null; #"
    substring 1 (stringLength str - 2) str;

  # Match the content of TOML format section names.
  ident_pat = ''[a-zA-Z0-9_-]+|"[^"]*"''; #"

  removeBraces = token: wrapLen:
    substring wrapLen (stringLength token - 2 * wrapLen) token;

  # Note, this implementation is limited to 11 identifiers.
  matchPathFun_1_11 = token:
    let
      # match header_pat "a.b.c" == [ "a" ".b" "b" ".c" "c" ]
      header_pat =
        foldl' (pat: n: "(${ident_pat})([.]${pat})?")
           "(${ident_pat})" (genList (n: 0) 10);
      matchPath = match header_pat token;
      filterDot = filter (s: substring 0 1 s != ".") matchPath;
    in
      filterDot;

  matchPathFun_1_12 = token:
    map (e: head e)
      (filter (s: isList s)
        (split "(${ident_pat})" token));

  matchPathFun =
    if builtins ? split
    then matchPathFun_1_12
    else matchPathFun_1_11;

  headerToPath = token: wrapLen:
    let
      token' = removeBraces token wrapLen;
      matchPath = matchPathFun token';
      path =
        map (s:
          if substring 0 1 s != ''"'' then s #"
          else unescapeString s 
        ) matchPath;
    in 
      assert matchPath != null;
      # assert trace "Path: ${token'}; match as ${toString path}" true;
      path;
in

# Reconstruct the equivalent attribute set.
let
  tokenToValue = token:
    if token == "true" then true
    else if token == "false" then false
    # TODO: convert the TOML list into a Nix list.
    else if match "[[][^]]+[]]" token != null then token
    else unescapeString token;

  parserInitState = {
    idx = 0;
    path = [];
    isList = false;
    output = [];
    elem = {};
  };

  # Imported from nixpkgs library.
  setAttrByPath = attrPath: value:
    if attrPath == [] then value
    else listToAttrs
      [ { name = head attrPath; value = setAttrByPath (tail attrPath) value; } ];

  closeSection = state:
    state // {
      output = state.output ++ [ (setAttrByPath state.path (
        if state.isList then [ state.elem ]
        else state.elem
      )) ];
    };

  readToken = state: token:
    # assert trace "Read '${token}'" true;
    if state.idx == 0 then
      if substring 0 2 token == "[[" then
        (closeSection state) // {
          path = headerToPath token 2;
          isList = true;
          elem = {};
        }
      else if substring 0 1 token == "[" then
        (closeSection state) // {
          path = headerToPath token 1;
          isList = false;
          elem = {};
        }
      else
        assert match "[a-zA-Z0-9_-]+" token != null;
        state // { idx = 1; name = token; }
    else if state.idx == 1 then
      assert token == "=";
      state // { idx = 2; }
    else
      assert state.idx == 2;
      state // {
        idx = 0;
        elem = state.elem // {
          "${state.name}" = tokenToValue token;
        };
      };

  # aggregate each section as individual attribute sets.
  parser = str:
    closeSection (foldl' readToken parserInitState (tokenizer str));

  fromTOML = toml:
    let
      sections = (parser toml).output;
      # Inlined from nixpkgs library functions.
      zipAttrs = sets:
        listToAttrs (map (n: {
          name = n;
          value =
            let v = catAttrs n sets; in
            # assert trace "Visiting ${n}" true;
            if tail v == [] then head v 
            else if isList (head v) then concatLists v
            else if isAttrs (head v) then zipAttrs v
            else throw "cannot merge sections";
        }) (concatLists (map attrNames sets)));
    in
      zipAttrs sections;
in

{
  testing = fromTOML (builtins.readFile ./channel-rust-nightly.toml);
  testing_url = fromTOML (builtins.readFile (builtins.fetchurl
  https://static.rust-lang.org/dist/channel-rust-nightly.toml));
  inherit fromTOML;
}

