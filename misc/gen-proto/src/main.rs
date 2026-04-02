use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// A single protobuf definition to compile.
struct ProtoDef {
    /// Path to the `.proto` file, relative to the workspace root.
    proto_file: &'static str,
    /// Include directory passed to `prost-build`, relative to the workspace root.
    include_dir: &'static str,
    /// Output directory for the generated `.rs` file, relative to the workspace root.
    output_dir: &'static str,
}

const PROTO_DEFS: &[ProtoDef] = &[
    // misc/prost-codec (test proto)
    ProtoDef {
        proto_file: "misc/prost-codec/src/generated/test.proto",
        include_dir: "misc/prost-codec/src/generated",
        output_dir: "misc/prost-codec/src/generated",
    },
    // core
    ProtoDef {
        proto_file: "core/src/generated/envelope.proto",
        include_dir: "core/src/generated",
        output_dir: "core/src/generated",
    },
    ProtoDef {
        proto_file: "core/src/generated/peer_record.proto",
        include_dir: "core/src/generated",
        output_dir: "core/src/generated",
    },
    // identity
    ProtoDef {
        proto_file: "identity/src/generated/keys.proto",
        include_dir: "identity/src/generated",
        output_dir: "identity/src/generated",
    },
    // webrtc-utils
    ProtoDef {
        proto_file: "misc/webrtc-utils/src/generated/message.proto",
        include_dir: "misc/webrtc-utils/src/generated",
        output_dir: "misc/webrtc-utils/src/generated",
    },
    // noise
    ProtoDef {
        proto_file: "transports/noise/src/generated/payload.proto",
        include_dir: "transports/noise/src/generated",
        output_dir: "transports/noise/src/generated",
    },
    // plaintext
    ProtoDef {
        proto_file: "transports/plaintext/src/generated/structs.proto",
        include_dir: "transports/plaintext/src/generated",
        output_dir: "transports/plaintext/src/generated",
    },
    // autonat v1
    ProtoDef {
        proto_file: "protocols/autonat/src/v1/generated/structs.proto",
        include_dir: "protocols/autonat/src/v1/generated",
        output_dir: "protocols/autonat/src/v1/generated",
    },
    // autonat v2
    ProtoDef {
        proto_file: "protocols/autonat/src/v2/generated/structs.proto",
        include_dir: "protocols/autonat/src/v2/generated",
        output_dir: "protocols/autonat/src/v2/generated",
    },
    // dcutr
    ProtoDef {
        proto_file: "protocols/dcutr/src/generated/message.proto",
        include_dir: "protocols/dcutr/src/generated",
        output_dir: "protocols/dcutr/src/generated",
    },
    // floodsub
    ProtoDef {
        proto_file: "protocols/floodsub/src/generated/rpc.proto",
        include_dir: "protocols/floodsub/src/generated",
        output_dir: "protocols/floodsub/src/generated",
    },
    // gossipsub (two proto files)
    ProtoDef {
        proto_file: "protocols/gossipsub/src/generated/rpc.proto",
        include_dir: "protocols/gossipsub/src/generated",
        output_dir: "protocols/gossipsub/src/generated",
    },
    ProtoDef {
        proto_file: "protocols/gossipsub/src/generated/compat.proto",
        include_dir: "protocols/gossipsub/src/generated",
        output_dir: "protocols/gossipsub/src/generated",
    },
    // identify
    ProtoDef {
        proto_file: "protocols/identify/src/generated/structs.proto",
        include_dir: "protocols/identify/src/generated",
        output_dir: "protocols/identify/src/generated",
    },
    // kad
    ProtoDef {
        proto_file: "protocols/kad/src/generated/dht.proto",
        include_dir: "protocols/kad/src/generated",
        output_dir: "protocols/kad/src/generated",
    },
    // relay
    ProtoDef {
        proto_file: "protocols/relay/src/generated/message.proto",
        include_dir: "protocols/relay/src/generated",
        output_dir: "protocols/relay/src/generated",
    },
    // rendezvous
    ProtoDef {
        proto_file: "protocols/rendezvous/src/generated/rpc.proto",
        include_dir: "protocols/rendezvous/src/generated",
        output_dir: "protocols/rendezvous/src/generated",
    },
];

fn main() {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();

    assert!(
        workspace_root.join("Cargo.toml").exists(),
        "Workspace root detection failed: {}",
        workspace_root.display()
    );

    println!("Workspace root: {}", workspace_root.display());

    // Group proto files by (include_dir, output_dir) so we can batch them
    // into single prost-build invocations where they share the same dirs.
    let mut groups: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();

    for def in PROTO_DEFS {
        groups
            .entry((def.include_dir, def.output_dir))
            .or_default()
            .push(def.proto_file);
    }

    for ((include_dir, output_dir), proto_files) in &groups {
        let abs_output = workspace_root.join(output_dir);
        let abs_include = workspace_root.join(include_dir);

        std::fs::create_dir_all(&abs_output).unwrap();

        let abs_protos: Vec<PathBuf> = proto_files
            .iter()
            .map(|p| workspace_root.join(p))
            .collect();

        println!(
            "Generating {} proto file(s) from {} -> {}",
            abs_protos.len(),
            include_dir,
            output_dir
        );

        prost_build::Config::new()
            .out_dir(&abs_output)
            .compile_protos(
                &abs_protos
                    .iter()
                    .map(|p| p.as_path())
                    .collect::<Vec<_>>(),
                &[abs_include.as_path()],
            )
            .unwrap_or_else(|e| {
                panic!("Failed to compile protos in {}: {}", include_dir, e)
            });
    }

    // Now generate mod.rs files for each output directory.
    let output_dirs: BTreeSet<&str> = PROTO_DEFS.iter().map(|def| def.output_dir).collect();

    for output_dir in &output_dirs {
        let abs_output = workspace_root.join(output_dir);
        generate_mod_rs(&abs_output);
    }

    println!("Done! All proto files generated successfully.");
}

fn generate_mod_rs(dir: &std::path::Path) {
    let mut modules = Vec::new();

    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().map_or(false, |ext| ext == "rs") {
            let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
            if stem != "mod" {
                modules.push(stem);
            }
        }
    }

    modules.sort();

    let mut content = String::from("// Automatically generated by gen-proto. DO NOT EDIT.\n// Regenerate with: cd misc/gen-proto && cargo run\n");
    for module in &modules {
        // prost generates files like `package.name.rs` - we need to handle dots
        // by using #[path = "..."] attribute
        if module.contains('.') {
            let mod_name = module.replace('.', "_");
            content.push_str(&format!(
                "#[path = \"{module}.rs\"]\npub mod {mod_name};\n"
            ));
        } else {
            content.push_str(&format!("pub mod {module};\n"));
        }
    }

    let mod_path = dir.join("mod.rs");
    std::fs::write(&mod_path, &content).unwrap();
    println!("  Generated {}", mod_path.display());
}
