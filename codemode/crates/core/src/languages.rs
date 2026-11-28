//! Declarative 40+ language registry, extension resolution, and LSP discovery.

use std::path::{Path, PathBuf};

use crate::config::ProjectConfig;

/// Canonical language identifiers covering all non-alias Serena `LanguageServerId` entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LanguageId {
    CSharp,
    VbNet,
    Python,
    Rust,
    Java,
    Kotlin,
    TypeScript,
    Go,
    Ruby,
    Dart,
    Cpp,
    Php,
    R,
    Perl,
    Clojure,
    Elixir,
    Elm,
    Terraform,
    Swift,
    Bash,
    Crystal,
    Cue,
    Zig,
    Lua,
    Luau,
    Nix,
    Erlang,
    Ocaml,
    Al,
    FSharp,
    Rego,
    Scala,
    Julia,
    Fortran,
    Haskell,
    Haxe,
    Lean4,
    Groovy,
    Vue,
    Svelte,
    Astro,
    PowerShell,
    Pascal,
    Matlab,
    Msl,
    Bsl,
    Ada,
    GdScript,
    Qml,
    Gleam,
    Nextflow,
    Wolfram,
    Markdown,
    Latex,
    Yaml,
    Json,
    Toml,
    Hlsl,
    SystemVerilog,
    Solidity,
    Ansible,
    Html,
    Scss,
    Angular,
    Deno,
    GraphQl,
}

/// Static metadata describing a supported language and its workspace/LSP conventions.
#[derive(Clone, Copy, Debug)]
pub struct LanguageSpec {
    pub id: LanguageId,
    pub key: &'static str,
    pub lsp_id: &'static str,
    pub extensions: &'static [&'static str],
    pub priority: u8,
    pub is_programming: bool,
    pub supports_implementation: bool,
    pub root_markers: &'static [&'static str],
    pub server_candidates: &'static [(&'static str, &'static [&'static str])],
}

const ALL_LANGUAGES: &[LanguageSpec] = &[
    LanguageSpec {
        id: LanguageId::CSharp,
        key: "csharp",
        lsp_id: "csharp",
        extensions: &["cs"],
        priority: 2,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["*.sln", "*.csproj", "global.json", ".git"],
        server_candidates: &[("csharp-ls", &["--stdio"]), ("OmniSharp", &["-lsp"])],
    },
    LanguageSpec {
        id: LanguageId::VbNet,
        key: "vbnet",
        lsp_id: "vb",
        extensions: &["vb", "cs"],
        priority: 1,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["*.sln", "*.vbproj", ".git"],
        server_candidates: &[("vbnet-language-server", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Python,
        key: "python",
        lsp_id: "python",
        extensions: &["py", "pyi"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["pyproject.toml", "setup.py", "setup.cfg", ".git"],
        server_candidates: &[
            ("ty", &["server"]),
            ("pyrefly", &["lsp"]),
            ("pyright-langserver", &["--stdio"]),
            ("basedpyright-langserver", &["--stdio"]),
            ("jedi-language-server", &[]),
        ],
    },
    LanguageSpec {
        id: LanguageId::Rust,
        key: "rust",
        lsp_id: "rust",
        extensions: &["rs"],
        priority: 2,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["Cargo.toml", ".git"],
        server_candidates: &[("rust-analyzer", &[])],
    },
    LanguageSpec {
        id: LanguageId::Java,
        key: "java",
        lsp_id: "java",
        extensions: &["java"],
        priority: 2,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["pom.xml", "build.gradle", "build.gradle.kts", ".git"],
        server_candidates: &[("jdtls", &[])],
    },
    LanguageSpec {
        id: LanguageId::Kotlin,
        key: "kotlin",
        lsp_id: "kotlin",
        extensions: &["kt", "kts"],
        priority: 2,
        is_programming: true,
        supports_implementation: true,
        root_markers: &[
            "build.gradle.kts",
            "build.gradle",
            "settings.gradle.kts",
            "settings.gradle",
            "pom.xml",
            ".git",
        ],
        server_candidates: &[
            ("kotlin-lsp", &["--stdio"]),
            ("kotlin-language-server", &["--stdio"]),
        ],
    },
    LanguageSpec {
        id: LanguageId::TypeScript,
        key: "typescript",
        lsp_id: "typescript",
        extensions: &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"],
        priority: 2,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["tsconfig.json", "jsconfig.json", "package.json", ".git"],
        server_candidates: &[
            ("tsgo", &["--lsp", "--stdio"]),
            ("vtsls", &["--stdio"]),
            ("typescript-language-server", &["--stdio"]),
        ],
    },
    LanguageSpec {
        id: LanguageId::Go,
        key: "go",
        lsp_id: "go",
        extensions: &["go"],
        priority: 2,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["go.mod", ".git"],
        server_candidates: &[("gopls", &[])],
    },
    LanguageSpec {
        id: LanguageId::Ruby,
        key: "ruby",
        lsp_id: "ruby",
        extensions: &["rb", "erb", "rake", "gemspec"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["Gemfile", "Rakefile", ".ruby-version", ".git"],
        server_candidates: &[("ruby-lsp", &[]), ("solargraph", &["stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Dart,
        key: "dart",
        lsp_id: "dart",
        extensions: &["dart"],
        priority: 2,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["pubspec.yaml", ".git"],
        server_candidates: &[("dart", &["language-server", "--protocol=lsp"])],
    },
    LanguageSpec {
        id: LanguageId::Cpp,
        key: "cpp",
        lsp_id: "cpp",
        extensions: &[
            "c", "h", "c++", "cc", "cp", "cpp", "cxx", "hh", "hpp", "hxx", "inl", "ipp", "tpp",
            "txx", "m", "mm", "c++m", "cppm", "cxxm", "ixx", "cu", "hip", "cl", "clcpp", "ino",
        ],
        priority: 2,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["compile_commands.json", "CMakeLists.txt", ".git"],
        server_candidates: &[("clangd", &[]), ("ccls", &[])],
    },
    LanguageSpec {
        id: LanguageId::Php,
        key: "php",
        lsp_id: "php",
        extensions: &["php", "phtml"],
        priority: 2,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["composer.json", ".git"],
        server_candidates: &[
            ("intelephense", &["--stdio"]),
            ("phpactor", &["language-server"]),
            ("phpantom_lsp", &[]),
        ],
    },
    LanguageSpec {
        id: LanguageId::R,
        key: "r",
        lsp_id: "r",
        extensions: &["r", "rmd", "rnw"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["DESCRIPTION", ".Rproj", ".git"],
        server_candidates: &[("R", &["--slave", "-e", "languageserver::run()"])],
    },
    LanguageSpec {
        id: LanguageId::Perl,
        key: "perl",
        lsp_id: "perl",
        extensions: &["pl", "pm", "t"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["cpanfile", "Makefile.PL", ".git"],
        server_candidates: &[("perlnavigator", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Clojure,
        key: "clojure",
        lsp_id: "clojure",
        extensions: &["clj", "cljs", "cljc", "edn"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["deps.edn", "project.clj", "shadow-cljs.edn", ".git"],
        server_candidates: &[("clojure-lsp", &[])],
    },
    LanguageSpec {
        id: LanguageId::Elixir,
        key: "elixir",
        lsp_id: "elixir",
        extensions: &["ex", "exs"],
        priority: 2,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["mix.exs", ".git"],
        server_candidates: &[
            ("expert", &["--stdio"]),
            ("elixir-ls", &[]),
            ("nextls", &["--stdio"]),
        ],
    },
    LanguageSpec {
        id: LanguageId::Elm,
        key: "elm",
        lsp_id: "elm",
        extensions: &["elm"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["elm.json", ".git"],
        server_candidates: &[("elm-language-server", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Terraform,
        key: "terraform",
        lsp_id: "terraform",
        extensions: &["tf", "tfvars", "tfstate"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &[".terraform.lock.hcl", "main.tf", ".git"],
        server_candidates: &[("terraform-ls", &["serve"])],
    },
    LanguageSpec {
        id: LanguageId::Swift,
        key: "swift",
        lsp_id: "swift",
        extensions: &["swift"],
        priority: 2,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["Package.swift", ".git"],
        server_candidates: &[("sourcekit-lsp", &[])],
    },
    LanguageSpec {
        id: LanguageId::Bash,
        key: "bash",
        lsp_id: "shellscript",
        extensions: &["sh", "bash"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &[".git"],
        server_candidates: &[("bash-language-server", &["start"])],
    },
    LanguageSpec {
        id: LanguageId::Crystal,
        key: "crystal",
        lsp_id: "crystal",
        extensions: &["cr"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["shard.yml", ".git"],
        server_candidates: &[("crystalline", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Cue,
        key: "cue",
        lsp_id: "cue",
        extensions: &["cue"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["cue.mod", ".git"],
        server_candidates: &[("cue", &["lsp"])],
    },
    LanguageSpec {
        id: LanguageId::Zig,
        key: "zig",
        lsp_id: "zig",
        extensions: &["zig", "zon"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["build.zig", "build.zig.zon", ".git"],
        server_candidates: &[("zls", &[])],
    },
    LanguageSpec {
        id: LanguageId::Lua,
        key: "lua",
        lsp_id: "lua",
        extensions: &["lua"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &[".luarc.json", ".luarc.jsonc", ".git"],
        server_candidates: &[("lua-language-server", &[]), ("emmylua_ls", &[])],
    },
    LanguageSpec {
        id: LanguageId::Luau,
        key: "luau",
        lsp_id: "luau",
        extensions: &["luau"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &[".luaurc", "default.project.json", ".git"],
        server_candidates: &[("luau-lsp", &["lsp"])],
    },
    LanguageSpec {
        id: LanguageId::Nix,
        key: "nix",
        lsp_id: "nix",
        extensions: &["nix"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["flake.nix", "default.nix", ".git"],
        server_candidates: &[("nixd", &[]), ("nil", &[])],
    },
    LanguageSpec {
        id: LanguageId::Erlang,
        key: "erlang",
        lsp_id: "erlang",
        extensions: &["erl", "hrl", "escript", "config", "app", "app.src"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["rebar.config", "erlang.mk", ".git"],
        server_candidates: &[("elp", &["server"]), ("erlang_ls", &[])],
    },
    LanguageSpec {
        id: LanguageId::Ocaml,
        key: "ocaml",
        lsp_id: "ocaml",
        extensions: &["ml", "mli", "re", "rei"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["dune-project", ".git"],
        server_candidates: &[("ocamllsp", &[])],
    },
    LanguageSpec {
        id: LanguageId::Al,
        key: "al",
        lsp_id: "al",
        extensions: &["al", "dal"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["app.json", ".git"],
        server_candidates: &[("al-language-server", &[])],
    },
    LanguageSpec {
        id: LanguageId::FSharp,
        key: "fsharp",
        lsp_id: "fsharp",
        extensions: &["fs", "fsx", "fsi"],
        priority: 2,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["*.sln", "*.fsproj", ".git"],
        server_candidates: &[("fsautocomplete", &["--adaptive-lsp-server-enabled"])],
    },
    LanguageSpec {
        id: LanguageId::Rego,
        key: "rego",
        lsp_id: "rego",
        extensions: &["rego"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &[".regal", ".git"],
        server_candidates: &[("regal", &["language-server"])],
    },
    LanguageSpec {
        id: LanguageId::Scala,
        key: "scala",
        lsp_id: "scala",
        extensions: &["scala", "sbt"],
        priority: 2,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["build.sbt", "build.sc", "pom.xml", ".git"],
        server_candidates: &[("metals", &[])],
    },
    LanguageSpec {
        id: LanguageId::Julia,
        key: "julia",
        lsp_id: "julia",
        extensions: &["jl"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["Project.toml", ".git"],
        server_candidates: &[(
            "julia",
            &[
                "--startup-file=no",
                "-e",
                "using LanguageServer; runserver()",
            ],
        )],
    },
    LanguageSpec {
        id: LanguageId::Fortran,
        key: "fortran",
        lsp_id: "fortran",
        extensions: &["f90", "f95", "f03", "f08", "f", "for", "fpp"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["fpm.toml", "CMakeLists.txt", ".git"],
        server_candidates: &[("fortls", &[])],
    },
    LanguageSpec {
        id: LanguageId::Haskell,
        key: "haskell",
        lsp_id: "haskell",
        extensions: &["hs", "lhs"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["hie.yaml", "cabal.project", "stack.yaml", "*.cabal", ".git"],
        server_candidates: &[("haskell-language-server-wrapper", &["--lsp"])],
    },
    LanguageSpec {
        id: LanguageId::Haxe,
        key: "haxe",
        lsp_id: "haxe",
        extensions: &["hx"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["hxml", "build.hxml", ".git"],
        server_candidates: &[("haxe-language-server", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Lean4,
        key: "lean4",
        lsp_id: "lean4",
        extensions: &["lean"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["lean-toolchain", "lakefile.toml", "lakefile.lean", ".git"],
        server_candidates: &[("lean", &["--server"])],
    },
    LanguageSpec {
        id: LanguageId::Groovy,
        key: "groovy",
        lsp_id: "groovy",
        extensions: &["groovy", "gvy"],
        priority: 0,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["build.gradle", "pom.xml", ".git"],
        server_candidates: &[("groovy-language-server", &[])],
    },
    LanguageSpec {
        id: LanguageId::Vue,
        key: "vue",
        lsp_id: "vue",
        extensions: &["vue", "ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"],
        priority: 1,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["vite.config.ts", "nuxt.config.ts", "package.json", ".git"],
        server_candidates: &[("vue-language-server", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Svelte,
        key: "svelte",
        lsp_id: "svelte",
        extensions: &["svelte", "ts", "mts", "cts", "js", "mjs", "cjs"],
        priority: 1,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["svelte.config.js", "package.json", ".git"],
        server_candidates: &[("svelteserver", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Astro,
        key: "astro",
        lsp_id: "astro",
        extensions: &["astro", "ts", "mts", "cts", "js", "mjs", "cjs"],
        priority: 1,
        is_programming: true,
        supports_implementation: true,
        root_markers: &[
            "astro.config.mjs",
            "astro.config.ts",
            "package.json",
            ".git",
        ],
        server_candidates: &[("astro-ls", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::PowerShell,
        key: "powershell",
        lsp_id: "powershell",
        extensions: &["ps1", "psm1", "psd1"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &[".git"],
        server_candidates: &[(
            "pwsh",
            &[
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "Import-Module PowerShellEditorServices",
            ],
        )],
    },
    LanguageSpec {
        id: LanguageId::Pascal,
        key: "pascal",
        lsp_id: "pascal",
        extensions: &["pas", "pp", "lpr", "dpr", "dpk", "inc"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &[".git"],
        server_candidates: &[("pasls", &[])],
    },
    LanguageSpec {
        id: LanguageId::Matlab,
        key: "matlab",
        lsp_id: "matlab",
        extensions: &["m", "mlx", "mlapp"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &[".git"],
        server_candidates: &[("matlab-language-server", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Msl,
        key: "msl",
        lsp_id: "msl",
        extensions: &["mrc"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &[".git"],
        server_candidates: &[("msl-language-server", &[])],
    },
    LanguageSpec {
        id: LanguageId::Bsl,
        key: "bsl",
        lsp_id: "bsl",
        extensions: &["bsl", "os"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &[".bsl-language-server.json", ".git"],
        server_candidates: &[("bsl-language-server", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Ada,
        key: "ada",
        lsp_id: "ada",
        extensions: &["ads", "adb", "ada"],
        priority: 2,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["*.gpr", "alire.toml", ".git"],
        server_candidates: &[("ada_language_server", &[])],
    },
    LanguageSpec {
        id: LanguageId::GdScript,
        key: "gdscript",
        lsp_id: "gdscript",
        extensions: &["gd", "gdscript"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["project.godot", ".git"],
        server_candidates: &[("godot", &["--headless", "--lsp-port", "6008"])],
    },
    LanguageSpec {
        id: LanguageId::Qml,
        key: "qml",
        lsp_id: "qml",
        extensions: &["qml"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["CMakeLists.txt", ".qmlls.ini", ".git"],
        server_candidates: &[("qmlls", &[]), ("qmlls6", &[])],
    },
    LanguageSpec {
        id: LanguageId::Gleam,
        key: "gleam",
        lsp_id: "gleam",
        extensions: &["gleam"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["gleam.toml", ".git"],
        server_candidates: &[("gleam", &["lsp"])],
    },
    LanguageSpec {
        id: LanguageId::Nextflow,
        key: "nextflow",
        lsp_id: "nextflow",
        extensions: &["nf"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["nextflow.config", ".git"],
        server_candidates: &[("nextflow-language-server", &[])],
    },
    LanguageSpec {
        id: LanguageId::Wolfram,
        key: "wolfram",
        lsp_id: "wolfram",
        extensions: &["wl", "wls"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &[".git"],
        server_candidates: &[("wolfram", &["-script", "-"])],
    },
    LanguageSpec {
        id: LanguageId::Markdown,
        key: "markdown",
        lsp_id: "markdown",
        extensions: &["md", "markdown"],
        priority: 0,
        is_programming: false,
        supports_implementation: false,
        root_markers: &[".git"],
        server_candidates: &[("marksman", &["server"])],
    },
    LanguageSpec {
        id: LanguageId::Latex,
        key: "latex",
        lsp_id: "latex",
        extensions: &["tex", "bib", "sty", "cls"],
        priority: 0,
        is_programming: false,
        supports_implementation: false,
        root_markers: &[".latexmkrc", ".git"],
        server_candidates: &[("texlab", &[])],
    },
    LanguageSpec {
        id: LanguageId::Yaml,
        key: "yaml",
        lsp_id: "yaml",
        extensions: &["yaml", "yml"],
        priority: 0,
        is_programming: false,
        supports_implementation: false,
        root_markers: &[".git"],
        server_candidates: &[("yaml-language-server", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Json,
        key: "json",
        lsp_id: "json",
        extensions: &["json", "jsonc"],
        priority: 0,
        is_programming: false,
        supports_implementation: false,
        root_markers: &[".git"],
        server_candidates: &[("vscode-json-language-server", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Toml,
        key: "toml",
        lsp_id: "toml",
        extensions: &["toml"],
        priority: 0,
        is_programming: false,
        supports_implementation: false,
        root_markers: &[".git"],
        server_candidates: &[("taplo", &["lsp", "stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Hlsl,
        key: "hlsl",
        lsp_id: "hlsl",
        extensions: &[
            "hlsl", "hlsli", "fx", "fxh", "cginc", "compute", "shader", "glsl", "vert", "frag",
            "geom", "tesc", "tese", "comp", "wgsl",
        ],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &[".git"],
        server_candidates: &[("shader-language-server", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::SystemVerilog,
        key: "systemverilog",
        lsp_id: "systemverilog",
        extensions: &["sv", "svh", "v", "vh"],
        priority: 2,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["verible.filelist", ".git"],
        server_candidates: &[("verible-verilog-ls", &[])],
    },
    LanguageSpec {
        id: LanguageId::Solidity,
        key: "solidity",
        lsp_id: "solidity",
        extensions: &["sol"],
        priority: 0,
        is_programming: true,
        supports_implementation: false,
        root_markers: &[
            "foundry.toml",
            "hardhat.config.js",
            "hardhat.config.ts",
            ".git",
        ],
        server_candidates: &[("nomicfoundation-solidity-language-server", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Ansible,
        key: "ansible",
        lsp_id: "ansible",
        extensions: &["yaml", "yml"],
        priority: 0,
        is_programming: false,
        supports_implementation: false,
        root_markers: &["ansible.cfg", "playbook.yml", ".git"],
        server_candidates: &[("ansible-language-server", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Html,
        key: "html",
        lsp_id: "html",
        extensions: &["html", "htm"],
        priority: 0,
        is_programming: false,
        supports_implementation: false,
        root_markers: &[".git"],
        server_candidates: &[("vscode-html-language-server", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Scss,
        key: "scss",
        lsp_id: "scss",
        extensions: &["scss", "sass", "css"],
        priority: 0,
        is_programming: true,
        supports_implementation: false,
        root_markers: &["package.json", ".git"],
        server_candidates: &[("some-sass-language-server", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Angular,
        key: "angular",
        lsp_id: "typescript",
        extensions: &["html", "htm", "ts", "tsx", "mts", "cts"],
        priority: 0,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["angular.json", "project.json", "package.json", ".git"],
        server_candidates: &[("ngserver", &["--stdio"])],
    },
    LanguageSpec {
        id: LanguageId::Deno,
        key: "deno",
        lsp_id: "typescript",
        extensions: &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"],
        priority: 0,
        is_programming: true,
        supports_implementation: true,
        root_markers: &["deno.json", "deno.jsonc", ".git"],
        server_candidates: &[("deno", &["lsp"])],
    },
    LanguageSpec {
        id: LanguageId::GraphQl,
        key: "graphql",
        lsp_id: "graphql",
        extensions: &["graphql", "gql"],
        priority: 0,
        is_programming: true,
        supports_implementation: false,
        root_markers: &[
            ".graphqlrc.yml",
            "graphql.config.yml",
            "graphql.config.json",
            ".git",
        ],
        server_candidates: &[("graphql-lsp", &["server", "-m", "stream"])],
    },
];

/// Returns all supported `LanguageSpec` entries in priority-resolution order.
pub fn all_languages() -> &'static [LanguageSpec] {
    ALL_LANGUAGES
}

impl LanguageId {
    /// Returns the Serena-compatible string key for this language.
    pub const fn key(self) -> &'static str {
        self.spec().key
    }

    /// Returns the static specification for this language.
    pub const fn spec(self) -> &'static LanguageSpec {
        let mut i = 0;
        while i < ALL_LANGUAGES.len() {
            if matches_id(ALL_LANGUAGES[i].id, self) {
                return &ALL_LANGUAGES[i];
            }
            i += 1;
        }
        panic!("missing LanguageSpec for LanguageId")
    }

    /// Returns the default LSP `languageId` for `didOpen`.
    pub const fn lsp_id(self) -> &'static str {
        self.spec().lsp_id
    }

    /// Refines the LSP `languageId` based on the concrete file extension.
    pub fn lsp_id_for_path(self, path: &Path) -> &'static str {
        let ext = normalized_ext(path).unwrap_or_default();
        match (self, ext.as_str()) {
            (Self::TypeScript | Self::Deno | Self::Angular, "tsx") => "typescriptreact",
            (Self::TypeScript | Self::Deno, "js" | "mjs" | "cjs") => "javascript",
            (Self::TypeScript | Self::Deno, "jsx") => "javascriptreact",
            (Self::Cpp, "c" | "h") => "c",
            (Self::Angular, "html" | "htm") => "html",
            (Self::Scss, "css") => "css",
            (Self::Scss, "sass") => "sass",
            _ => self.lsp_id(),
        }
    }

    /// Parses a language key or common alias (case-insensitive).
    pub fn from_key(raw: &str) -> Option<Self> {
        let norm = raw.trim().to_ascii_lowercase();
        if let Some(found) = ALL_LANGUAGES.iter().find(|spec| spec.key == norm) {
            return Some(found.id);
        }
        match norm.as_str() {
            "c" | "c++" | "cpp_ccls" => Some(Self::Cpp),
            "cs" | "csharp_omnisharp" => Some(Self::CSharp),
            "ts" | "js" | "javascript" | "tsx" | "typescript_vts" | "typescript_tsgo" => {
                Some(Self::TypeScript)
            }
            "py" | "python_jedi" | "python_ty" | "python_pyrefly" | "python_basedpyright" => {
                Some(Self::Python)
            }
            "lean" => Some(Self::Lean4),
            "ps1" | "pwsh" => Some(Self::PowerShell),
            "css" | "sass" => Some(Self::Scss),
            "sh" | "shell" => Some(Self::Bash),
            "php_phpactor" | "php_phpantom" | "php_devsense" => Some(Self::Php),
            "ruby_solargraph" => Some(Self::Ruby),
            "lua_emmylua" => Some(Self::Lua),
            "julia_fatou" => Some(Self::Julia),
            _ => None,
        }
    }

    /// Returns whether `path` has a matching extension or compound suffix (e.g. `.app.src`).
    pub fn matches_path(self, path: &Path) -> bool {
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        let lower_name = file_name.to_ascii_lowercase();
        let ext = normalized_ext(path);
        self.spec().extensions.iter().any(|candidate| {
            if candidate.contains('.') {
                lower_name.ends_with(&format!(".{candidate}"))
            } else {
                ext.as_deref() == Some(*candidate)
            }
        })
    }
}

const fn matches_id(a: LanguageId, b: LanguageId) -> bool {
    (a as u16) == (b as u16)
}

fn normalized_ext(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
}

/// Resolves the language for `path`, applying `.codemode/config.toml` overrides first.
pub fn resolve_language(path: &Path, config: Option<&ProjectConfig>) -> Option<LanguageId> {
    if let Some(cfg) = config {
        for configured in &cfg.languages {
            if let Some(lang) = LanguageId::from_key(configured)
                && lang.matches_path(path)
            {
                return Some(lang);
            }
        }
    }

    ALL_LANGUAGES
        .iter()
        .enumerate()
        .filter(|(_, spec)| spec.id.matches_path(path))
        .max_by(|(idx_a, spec_a), (idx_b, spec_b)| {
            spec_a
                .priority
                .cmp(&spec_b.priority)
                .then_with(|| idx_b.cmp(idx_a))
        })
        .map(|(_, spec)| spec.id)
}

fn dir_has_marker(dir: &Path, marker: &str) -> bool {
    if let Some(ext) = marker.strip_prefix("*.") {
        return std::fs::read_dir(dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case(ext))
            });
    }
    dir.join(marker).exists()
}

/// Finds the nearest directory owning `file` by language markers, `.codemode`, or `.git`.
pub fn workspace_root_for(file: &Path, lang: LanguageId) -> PathBuf {
    let start = if file.is_dir() {
        file
    } else {
        file.parent().unwrap_or(file)
    };
    let markers = lang.spec().root_markers;
    for dir in start.ancestors() {
        if markers.iter().any(|marker| dir_has_marker(dir, marker))
            || dir.join(".codemode").is_dir()
        {
            return dir.to_path_buf();
        }
    }
    start.to_path_buf()
}

fn find_binary(binary: &str) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH")
        && let Some(found) = std::env::split_paths(&path)
            .map(|dir| dir.join(binary))
            .find(|candidate| candidate.is_file())
    {
        return Some(found);
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        for sub in [".cargo/bin", ".elan/bin", ".local/bin"] {
            let candidate = home.join(sub).join(binary);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Resolves the language server command from config overrides, env vars, or system binaries.
pub fn server_command_for(
    lang: LanguageId,
    config: Option<&ProjectConfig>,
) -> Result<Vec<String>, String> {
    if let Some(cfg) = config
        && let Some(custom) = cfg.lsp_overrides.get(lang.key())
    {
        let parts: Vec<String> = custom.split_whitespace().map(str::to_string).collect();
        if !parts.is_empty() {
            return Ok(parts);
        }
    }

    let upper = lang.key().to_ascii_uppercase();
    for prefix in ["CODEMODE_LSP_", "ASTLENS_LSP_"] {
        let env_key = format!("{prefix}{upper}");
        if let Ok(custom) = std::env::var(&env_key) {
            let parts: Vec<String> = custom.split_whitespace().map(str::to_string).collect();
            if !parts.is_empty() {
                return Ok(parts);
            }
        }
    }

    let candidates = lang.spec().server_candidates;
    for (binary, args) in candidates {
        if let Some(path) = find_binary(binary) {
            let mut command = vec![path.display().to_string()];
            command.extend(args.iter().map(|arg| (*arg).to_string()));
            return Ok(command);
        }
    }

    let names: Vec<&str> = candidates.iter().map(|(binary, _)| *binary).collect();
    Err(format!(
        "no language server for {} on PATH (tried {}); install one or set CODEMODE_LSP_{upper}",
        lang.key(),
        names.join(", "),
    ))
}
