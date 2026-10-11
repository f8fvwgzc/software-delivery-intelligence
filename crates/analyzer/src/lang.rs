//! File extension → language id, shared by the code browser (tree and file
//! captions) and anything else that wants to name a file's language.
//!
//! Ids are the lowercase keys the rest of the analyzer uses (`rust`,
//! `typescript`, `csharp`, `shell`); a few web highlighter grammars that are
//! not languages of their own (`tsx`, `jsx`, `mdx`) keep their grammar name
//! because that is what a renderer needs. Unknown files answer `""`, never a
//! guess.

use crate::util::{basename, extension};

/// Lowercase language id for `path`, or `""` when the extension (or the
/// well-known file name) says nothing.
pub fn language_for_path(path: &str) -> &'static str {
    let name = basename(path);
    if let Some(lang) = by_name(name) {
        return lang;
    }
    by_extension(&extension(path))
}

/// Files whose name, not extension, says what they are.
fn by_name(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    let lang = match lower.as_str() {
        "dockerfile" | "containerfile" => "dockerfile",
        "makefile" | "gnumakefile" => "makefile",
        "cmakelists.txt" => "cmake",
        "jenkinsfile" => "groovy",
        "gemfile" | "gemfile.lock" | "rakefile" | "vagrantfile" | "brewfile" | "podfile"
        | "fastfile" | "guardfile" => "ruby",
        "cargo.lock" | "poetry.lock" | "pipfile" => "toml",
        "pipfile.lock" | "composer.lock" | ".babelrc" | ".eslintrc" | ".prettierrc" | ".swcrc" => {
            "json"
        }
        ".bashrc" | ".bash_profile" | ".zshrc" | ".zprofile" | ".profile" => "shell",
        ".gitignore" | ".dockerignore" | ".npmignore" | ".eslintignore" | ".prettierignore"
        | ".gitattributes" => "ignore",
        "go.mod" => "go-mod",
        "go.sum" => "go-sum",
        "build.gradle.kts" | "settings.gradle.kts" => "kotlin",
        _ => {
            if lower.starts_with("dockerfile.") || lower.ends_with(".dockerfile") {
                "dockerfile"
            } else {
                return None;
            }
        }
    };
    Some(lang)
}

fn by_extension(ext: &str) -> &'static str {
    match ext {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "jsx",
        "py" | "pyi" | "pyw" => "python",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "cs" | "csx" => "csharp",
        "fs" | "fsi" | "fsx" => "fsharp",
        "vb" => "vb",
        "rb" | "rake" | "gemspec" | "ru" => "ruby",
        "php" | "phtml" => "php",
        "ex" | "exs" => "elixir",
        "erl" | "hrl" => "erlang",
        "swift" => "swift",
        "scala" | "sbt" | "sc" => "scala",
        "dart" => "dart",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++" | "ino" => "cpp",
        "m" => "objective-c",
        "mm" => "objective-cpp",
        "sh" | "bash" | "zsh" | "ksh" | "fish" => "shell",
        "ps1" | "psm1" | "psd1" => "powershell",
        "bat" | "cmd" => "batch",
        "html" | "htm" | "xhtml" => "html",
        "css" => "css",
        "scss" => "scss",
        "sass" => "sass",
        "less" => "less",
        "styl" => "stylus",
        "vue" => "vue",
        "svelte" => "svelte",
        "astro" => "astro",
        "json" | "jsonc" | "json5" | "webmanifest" | "geojson" | "har" => "json",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "ini" | "cfg" | "conf" | "editorconfig" | "properties" | "env" => "ini",
        "xml" | "xsd" | "xsl" | "xslt" | "plist" | "csproj" | "fsproj" | "vbproj" | "props"
        | "targets" | "nuspec" | "pom" | "wsdl" | "svg" | "storyboard" | "xib" => "xml",
        "md" | "markdown" | "mkd" => "markdown",
        "mdx" => "mdx",
        "rst" => "rst",
        "adoc" | "asciidoc" => "asciidoc",
        "tex" | "sty" | "cls" => "latex",
        "txt" | "text" | "log" => "plaintext",
        "csv" | "tsv" => "csv",
        "sql" | "ddl" | "dml" | "psql" | "mysql" | "pgsql" => "sql",
        "graphql" | "gql" => "graphql",
        "proto" => "protobuf",
        "tf" | "tfvars" | "hcl" | "nomad" | "workflow" => "hcl",
        "dockerfile" => "dockerfile",
        "mk" | "mak" => "makefile",
        "cmake" => "cmake",
        "gradle" | "groovy" | "gvy" | "gy" | "gsh" => "groovy",
        "lua" => "lua",
        "r" | "rmd" => "r",
        "pl" | "pm" | "t" | "pod" => "perl",
        "hs" | "lhs" => "haskell",
        "ml" | "mli" => "ocaml",
        "clj" | "cljs" | "cljc" | "edn" => "clojure",
        "elm" => "elm",
        "jl" => "julia",
        "nim" | "nims" => "nim",
        "zig" => "zig",
        "nix" => "nix",
        "v" | "sv" | "svh" => "verilog",
        "vhd" | "vhdl" => "vhdl",
        "sol" => "solidity",
        "wat" => "wasm",
        "diff" | "patch" => "diff",
        "ipynb" => "json",
        "cue" => "cue",
        "jsonnet" | "libsonnet" => "jsonnet",
        "dhall" => "dhall",
        "bicep" => "bicep",
        "prisma" => "prisma",
        "ejs" | "hbs" | "handlebars" | "mustache" | "njk" | "jinja" | "jinja2" | "j2" | "twig"
        | "liquid" | "erb" | "haml" | "slim" | "pug" | "jade" => "template",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_extensions() {
        assert_eq!(language_for_path("src/main.rs"), "rust");
        assert_eq!(language_for_path("web/src/App.tsx"), "tsx");
        assert_eq!(language_for_path("web/src/lib/api.ts"), "typescript");
        assert_eq!(language_for_path("scripts/build.MJS"), "javascript");
        assert_eq!(language_for_path("app/models.py"), "python");
        assert_eq!(language_for_path("Program.cs"), "csharp");
        assert_eq!(language_for_path("infra/main.tf"), "hcl");
        assert_eq!(language_for_path("README.md"), "markdown");
        assert_eq!(language_for_path("proto/sdi/v1/x.proto"), "protobuf");
        assert_eq!(language_for_path("deploy/nginx/nginx.conf"), "ini");
    }

    #[test]
    fn maps_well_known_file_names() {
        assert_eq!(language_for_path("Dockerfile"), "dockerfile");
        assert_eq!(language_for_path("deploy/Dockerfile.web"), "dockerfile");
        assert_eq!(language_for_path("Makefile"), "makefile");
        assert_eq!(language_for_path("Gemfile"), "ruby");
        assert_eq!(language_for_path("Cargo.lock"), "toml");
        assert_eq!(language_for_path("CMakeLists.txt"), "cmake");
        assert_eq!(language_for_path(".gitignore"), "ignore");
        assert_eq!(language_for_path("go.mod"), "go-mod");
    }

    #[test]
    fn unknown_files_are_empty() {
        assert_eq!(language_for_path("LICENSE"), "");
        assert_eq!(language_for_path("assets/logo.png"), "");
        assert_eq!(language_for_path("bin/tool"), "");
        assert_eq!(language_for_path(""), "");
        assert_eq!(language_for_path("weird.unknownext"), "");
    }
}
