use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct TarballInfo {
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct DistroPlugin {
    pub alias: String,
    pub name: String,
    pub comment: Option<String>,
    pub tarballs: HashMap<String, TarballInfo>,
    pub has_setup: bool,
}

impl DistroPlugin {
    pub fn supported_architectures(&self) -> Vec<&str> {
        let mut archs: Vec<&str> = self.tarballs.keys().map(|s| s.as_str()).collect();
        archs.sort();
        archs
    }
}

impl fmt::Display for DistroPlugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.name)?;
        if let Some(ref comment) = self.comment {
            writeln!(f, "  comment: {}", comment)?;
        }
        let archs = self.supported_architectures().join(", ");
        writeln!(f, "  archs:    {}", archs)?;
        write!(
            f,
            "  setup:    {}",
            if self.has_setup { "yes" } else { "no" }
        )
    }
}

#[derive(Debug)]
pub enum ParseError {
    Io(io::Error),
    MissingField { field: String, file: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Io(e) => write!(f, "IO error: {}", e),
            ParseError::MissingField { field, file } => {
                write!(f, "missing {} in {}", field, file)
            }
        }
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ParseError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for ParseError {
    fn from(e: io::Error) -> Self {
        ParseError::Io(e)
    }
}

fn parse_alias(path: &Path) -> String {
    let file_name = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default()
        .to_string();
    if let Some(alias) = file_name.strip_suffix(".override.sh") {
        return alias.to_string();
    }
    if let Some(alias) = file_name.strip_suffix(".sh") {
        return alias.to_string();
    }
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

pub fn parse_plugin(path: &Path) -> Result<DistroPlugin, ParseError> {
    let content = fs::read_to_string(path)?;
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let alias = parse_alias(path);

    let mut name = None;
    let mut comment = None;
    let mut has_setup = false;
    let mut urls: HashMap<String, String> = HashMap::new();
    let mut sha256s: HashMap<String, String> = HashMap::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        if let Some((key, value)) = parse_assignment(trimmed) {
            if key == "DISTRO_NAME" {
                name = Some(value.to_string());
            } else if key == "DISTRO_COMMENT" {
                comment = Some(value.to_string());
            } else if let Some(arch) = key.strip_prefix("TARBALL_URL_") {
                urls.insert(arch.to_string(), value.to_string());
            } else if let Some(arch) = key.strip_prefix("TARBALL_SHA256_") {
                sha256s.insert(arch.to_string(), value.to_string());
            } else if let Some(rest) = key.strip_prefix("TARBALL_URL[") {
                if let Some(arch) = rest.strip_suffix("]") {
                    urls.insert(arch.to_string(), value.to_string());
                }
            } else if let Some(rest) = key.strip_prefix("TARBALL_SHA256[") {
                if let Some(arch) = rest.strip_suffix("]") {
                    sha256s.insert(arch.to_string(), value.to_string());
                }
            }
        }

        if trimmed.starts_with("distro_setup()") || trimmed.starts_with("distro_setup (") {
            has_setup = true;
        }
    }

    let name = name.ok_or_else(|| ParseError::MissingField {
        field: "DISTRO_NAME".to_string(),
        file: file_name.clone(),
    })?;

    if urls.is_empty() {
        return Err(ParseError::MissingField {
            field: "TARBALL_URL_*".to_string(),
            file: file_name,
        });
    }

    let mut tarballs = HashMap::new();
    for (arch, url) in &urls {
        let sha256 = sha256s.get(arch).cloned().unwrap_or_default();
        tarballs.insert(
            arch.clone(),
            TarballInfo {
                url: url.clone(),
                sha256,
            },
        );
    }

    Ok(DistroPlugin {
        alias,
        name,
        comment,
        tarballs,
        has_setup,
    })
}

fn parse_assignment(line: &str) -> Option<(&str, &str)> {
    let eq_pos = line.find('=')?;
    let key = &line[..eq_pos];
    let rest = &line[eq_pos + 1..];

    if !is_valid_key(key) {
        return None;
    }

    let value = extract_quoted_value(rest)?;
    Some((key, value))
}

fn is_valid_key(key: &str) -> bool {
    if key.is_empty() {
        return false;
    }
    key.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '[' || c == ']')
}

fn extract_quoted_value(s: &str) -> Option<&str> {
    let trimmed = s.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        Some(&trimmed[1..trimmed.len() - 1])
    } else {
        None
    }
}

pub fn load_plugins(dir: &Path) -> Vec<DistroPlugin> {
    let mut by_alias: BTreeMap<String, DistroPlugin> = builtin_plugins()
        .into_iter()
        .map(|plugin| (plugin.alias.clone(), plugin))
        .collect();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or_default();
            if !(name.ends_with(".sh") || name.ends_with(".override.sh")) {
                continue;
            }
            if let Ok(plugin) = parse_plugin(&path) {
                by_alias.insert(plugin.alias.clone(), plugin);
            }
        }
    }

    by_alias.into_values().collect()
}

fn tarballs(items: &[(&str, &str, &str)]) -> HashMap<String, TarballInfo> {
    let mut map = HashMap::new();
    for (arch, url, sha256) in items {
        map.insert(
            (*arch).to_string(),
            TarballInfo {
                url: (*url).to_string(),
                sha256: (*sha256).to_string(),
            },
        );
    }
    map
}

fn builtin_plugins() -> Vec<DistroPlugin> {
    vec![
        DistroPlugin {
            alias: "alpine".to_string(),
            name: "Alpine Linux".to_string(),
            comment: Some("Regular release v3.23.3.".to_string()),
            tarballs: tarballs(&[
                ("aarch64", "https://easycli.sh/proot-distro/alpine-aarch64-pd-v4.37.0.tar.xz", "2bdfb03eae53e6163695f4cd3b86e67ddca78466c879a140e069b1263150599b"),
                ("arm", "https://easycli.sh/proot-distro/alpine-arm-pd-v4.37.0.tar.xz", "0d1bc9bb24f1efd3a95e22e04e3590f4adfb0ff1ff39bbc82281ccf12fc0916d"),
                ("i686", "https://easycli.sh/proot-distro/alpine-i686-pd-v4.37.0.tar.xz", "83004e9ae904d79d95a4ea367a6a8201827f4ab5d43b8c2318763a2e2fd4a9b1"),
                ("riscv64", "https://easycli.sh/proot-distro/alpine-riscv64-pd-v4.37.0.tar.xz", "40e3c388d0bf7cac449903f0b838cbb5b0caac767946b257cd70f1f59ba76dc3"),
                ("x86_64", "https://easycli.sh/proot-distro/alpine-x86_64-pd-v4.37.0.tar.xz", "3cc015ee38585ae6e933b036e9d8532d2e77e548bd1b7b05f81828d30e3a3606"),
            ]),
            has_setup: false,
        },
        DistroPlugin {
            alias: "archlinux".to_string(),
            name: "Arch Linux".to_string(),
            comment: Some("ARM(64) devices use Arch Linux ARM, i686 uses Arch Linux 32. Both are independent projects. The original Arch usable only by x86_64 devices.".to_string()),
            tarballs: tarballs(&[
                ("aarch64", "https://easycli.sh/proot-distro/archlinux-aarch64-pd-v4.37.0.tar.xz", "718151cc4adad701223c689a7e4690cb7710b7b16e9b23617b671856ff04d563"),
                ("arm", "https://easycli.sh/proot-distro/archlinux-arm-pd-v4.37.0.tar.xz", "abc5d7d135db40a9e27a724553101b6ea13341e084cbb8b1d38befd9088f88bc"),
                ("i686", "https://easycli.sh/proot-distro/archlinux-i686-pd-v4.37.0.tar.xz", "7997c0f1a294585f571a4adf619690762130dfee0b43333458c763270666e979"),
                ("x86_64", "https://easycli.sh/proot-distro/archlinux-x86_64-pd-v4.37.0.tar.xz", "ebff09d2603f25205f1d8a2bd05b132fde571dd32f2ee58638f3a8dd8735282d"),
            ]),
            has_setup: false,
        },
        DistroPlugin {
            alias: "debian".to_string(),
            name: "Debian (trixie)".to_string(),
            comment: Some("Stable release.".to_string()),
            tarballs: tarballs(&[
                ("aarch64", "https://easycli.sh/proot-distro/debian-trixie-aarch64-pd-v4.37.0.tar.xz", "9bd3b19ff7cd300c7c7bf33124b726eb199f4bab9a3b1472f34749c6d12c9195"),
                ("arm", "https://easycli.sh/proot-distro/debian-trixie-arm-pd-v4.37.0.tar.xz", "af9b22fc1b82ccc665e484342af71c35a86f9f3dd525b0f423649976dded239f"),
                ("i686", "https://easycli.sh/proot-distro/debian-trixie-i686-pd-v4.37.0.tar.xz", "61f4c3b55d5defc1e9885efbe3b78d476f30d146eaffe45030916a77341c6768"),
                ("x86_64", "https://easycli.sh/proot-distro/debian-trixie-x86_64-pd-v4.37.0.tar.xz", "17eec851f40330cb3be77880aedd9e49c87d044f4ee5b02b3568c6aae0a5973b"),
            ]),
            has_setup: false,
        },
        DistroPlugin {
            alias: "fedora".to_string(),
            name: "Fedora".to_string(),
            comment: Some("Version 43. Broken on Android 15+.".to_string()),
            tarballs: tarballs(&[
                ("aarch64", "https://easycli.sh/proot-distro/fedora-aarch64-pd-v4.37.0.tar.xz", "eb86202ef9887dc315e93c627bef3b6a825da871129ab3de91466ab2c2e06019"),
                ("x86_64", "https://easycli.sh/proot-distro/fedora-x86_64-pd-v4.37.0.tar.xz", "0daac2fe47dbfcdbcc89e8e92c7a59db4a3c78b3c226e4b4a04e6c2ec582bfd4"),
            ]),
            has_setup: false,
        },
        DistroPlugin {
            alias: "manjaro".to_string(),
            name: "Manjaro".to_string(),
            comment: Some("Manjaro ARM64 port.".to_string()),
            tarballs: tarballs(&[
                ("aarch64", "https://easycli.sh/proot-distro/manjaro-aarch64-pd-v4.37.0.tar.xz", "90fd86130d440b6d6ed6408b21306189eb41fe07d0026aab836ae203a1c419a4"),
            ]),
            has_setup: false,
        },
        DistroPlugin {
            alias: "opensuse".to_string(),
            name: "OpenSUSE".to_string(),
            comment: Some("Leap release (16.0). No support for ARM and x86 32bit.".to_string()),
            tarballs: tarballs(&[
                ("aarch64", "https://easycli.sh/proot-distro/opensuse-aarch64-pd-v4.37.0.tar.xz", "812bbed638f43b81846520bf4283c18da08e19f14714e56fffdc9ccad3c65d7a"),
                ("x86_64", "https://easycli.sh/proot-distro/opensuse-x86_64-pd-v4.37.0.tar.xz", "56cd4b5bb298da2ad25d66ec5f180c0f577c7f70358f323c62c318f8b8530ff7"),
            ]),
            has_setup: false,
        },
        DistroPlugin {
            alias: "rockylinux".to_string(),
            name: "Rocky Linux".to_string(),
            comment: Some("Version 10.1".to_string()),
            tarballs: tarballs(&[
                ("aarch64", "https://easycli.sh/proot-distro/rocky-aarch64-pd-v4.37.0.tar.xz", "0282a82a75e0b17aa0f72622847ee0bfda85fa84bb6cf49bc72c5515816c47f0"),
                ("x86_64", "https://easycli.sh/proot-distro/rocky-x86_64-pd-v4.37.0.tar.xz", "3d092c49815aadcd3607fb5a3dd4781def0832e251d1f342d83a69a5dde42582"),
            ]),
            has_setup: false,
        },
        DistroPlugin {
            alias: "ubuntu".to_string(),
            name: "Ubuntu (25.10)".to_string(),
            comment: Some("Regular release (questing).".to_string()),
            tarballs: tarballs(&[
                ("aarch64", "https://easycli.sh/proot-distro/ubuntu-questing-aarch64-pd-v4.37.0.tar.xz", "37e61ce5fd8593a7d10c4e72ebe611adb7e795f7492e4c0bf3a950441c984161"),
                ("arm", "https://easycli.sh/proot-distro/ubuntu-questing-arm-pd-v4.37.0.tar.xz", "8909d0942506792f08d0075341d3d5c9b6e6b2c14839082894db8878214d8a95"),
                ("x86_64", "https://easycli.sh/proot-distro/ubuntu-questing-x86_64-pd-v4.37.0.tar.xz", "0fe0add7dff6adeaa58d5a6f44225cedf1924bd6c221c886077fa3b595319c2d"),
            ]),
            has_setup: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_assignment_simple() {
        let (key, val) = parse_assignment(r#"DISTRO_NAME="Alpine Linux""#).unwrap();
        assert_eq!(key, "DISTRO_NAME");
        assert_eq!(val, "Alpine Linux");
    }

    #[test]
    fn test_parse_alias_for_override_file() {
        let tmp = std::env::temp_dir().join("debian.override.sh");
        assert_eq!(parse_alias(&tmp), "debian");
    }

    #[test]
    fn test_parse_assignment_with_arch() {
        let (key, val) =
            parse_assignment(r#"TARBALL_URL_aarch64="https://example.com/alpine.tar.xz""#).unwrap();
        assert_eq!(key, "TARBALL_URL_aarch64");
        assert_eq!(val, "https://example.com/alpine.tar.xz");
    }

    #[test]
    fn test_parse_assignment_sha256() {
        let (key, val) = parse_assignment(
            r#"TARBALL_SHA256_aarch64="2bdfb03eae53e6163695f4cd3b86e67ddca78466c879a140e069b1263150599b""#,
        )
        .unwrap();
        assert_eq!(key, "TARBALL_SHA256_aarch64");
        assert_eq!(
            val,
            "2bdfb03eae53e6163695f4cd3b86e67ddca78466c879a140e069b1263150599b"
        );
    }

    #[test]
    fn test_parse_assignment_comment_line() {
        assert!(parse_assignment("# This is a comment").is_none());
    }

    #[test]
    fn test_parse_assignment_empty_line() {
        assert!(parse_assignment("").is_none());
    }

    #[test]
    fn test_parse_assignment_function_def() {
        assert!(parse_assignment("distro_setup() {").is_none());
    }

    #[test]
    fn test_extract_quoted_value() {
        assert_eq!(extract_quoted_value(r#""hello""#), Some("hello"));
        assert_eq!(extract_quoted_value(r#""""#), Some(""));
        assert_eq!(extract_quoted_value("noquotes"), None);
        assert_eq!(extract_quoted_value(r#""unclosed"#), None);
    }

    #[test]
    fn test_detect_distro_setup() {
        let content = r#"
DISTRO_NAME="Arch Linux"
TARBALL_URL_aarch64="https://example.com/arch.tar.xz"
TARBALL_SHA256_aarch64="abc123"

distro_setup() {
    echo "hello"
}
"#;
        let tmp = std::env::temp_dir().join("test_plugin_setup.sh");
        fs::write(&tmp, content).unwrap();
        let plugin = parse_plugin(&tmp).unwrap();
        assert!(plugin.has_setup);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_no_distro_setup() {
        let content = r#"
DISTRO_NAME="Alpine Linux"
TARBALL_URL_aarch64="https://example.com/alpine.tar.xz"
TARBALL_SHA256_aarch64="abc123"
"#;
        let tmp = std::env::temp_dir().join("test_plugin_no_setup.sh");
        fs::write(&tmp, content).unwrap();
        let plugin = parse_plugin(&tmp).unwrap();
        assert!(!plugin.has_setup);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_missing_distro_name() {
        let content = r#"
TARBALL_URL_aarch64="https://example.com/test.tar.xz"
TARBALL_SHA256_aarch64="abc123"
"#;
        let tmp = std::env::temp_dir().join("test_plugin_no_name.sh");
        fs::write(&tmp, content).unwrap();
        let result = parse_plugin(&tmp);
        assert!(result.is_err());
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_missing_tarball_url() {
        let content = r#"DISTRO_NAME="Empty Distro""#;
        let tmp = std::env::temp_dir().join("test_plugin_no_tarball.sh");
        fs::write(&tmp, content).unwrap();
        let result = parse_plugin(&tmp);
        assert!(result.is_err());
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_supported_architectures() {
        let content = r#"
DISTRO_NAME="Test"
TARBALL_URL_aarch64="https://example.com/a.tar.xz"
TARBALL_SHA256_aarch64="aaa"
TARBALL_URL_x86_64="https://example.com/b.tar.xz"
TARBALL_SHA256_x86_64="bbb"
TARBALL_URL_arm="https://example.com/c.tar.xz"
TARBALL_SHA256_arm="ccc"
"#;
        let tmp = std::env::temp_dir().join("test_plugin_archs.sh");
        fs::write(&tmp, content).unwrap();
        let plugin = parse_plugin(&tmp).unwrap();
        let archs = plugin.supported_architectures();
        assert_eq!(archs, vec!["aarch64", "arm", "x86_64"]);
        let _ = fs::remove_file(&tmp);
    }
}
