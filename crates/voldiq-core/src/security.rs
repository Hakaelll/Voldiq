//! Conservative context from recorded/native path names only. No file or content access.
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveKind {
    CredentialLocation,
    CredentialFile,
    KeyMaterial,
    PasswordStore,
    PrivateApplicationState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SensitiveWarning {
    pub kind: SensitiveKind,
    pub reason: &'static str,
}

/// Absence of a warning is not evidence that a file is safe or contains no secrets.
/// The same detection excludes candidates and exact cleanup plans across adapters.
pub fn warning(path: &Path) -> Option<SensitiveWarning> {
    for part in path.components() {
        let name = part.as_os_str().to_string_lossy().to_ascii_lowercase();
        let kind = match name.as_str() {
            ".ssh" | ".aws" | ".azure" | ".gnupg" | ".kube" | ".docker" => {
                SensitiveKind::CredentialLocation
            }
            ".codex" => SensitiveKind::PrivateApplicationState,
            ".env"
            | ".netrc"
            | "_netrc"
            | ".npmrc"
            | ".pypirc"
            | ".git-credentials"
            | "credentials"
            | "credentials.json"
            | "credentials.ini"
            | "credentials.yml"
            | "credentials.yaml"
            | "auth.json"
            | "token.json"
            | "tokens.json"
            | "secrets.json"
            | "secrets.yml"
            | "secrets.yaml"
            | "passwords.txt"
            | "passwords.csv"
            | "service-account.json" => SensitiveKind::CredentialFile,
            "id_rsa" | "id_dsa" | "id_ecdsa" | "id_ed25519" | "id_ecdsa_sk" | "id_ed25519_sk" => {
                SensitiveKind::KeyMaterial
            }
            "login data" | "key4.db" | "logins.json" => SensitiveKind::PasswordStore,
            _ if name.starts_with(".env.") => SensitiveKind::CredentialFile,
            _ if [".pem", ".key", ".p12", ".pfx", ".ppk"]
                .iter()
                .any(|suffix| name.ends_with(suffix)) =>
            {
                SensitiveKind::KeyMaterial
            }
            _ if name.ends_with(".kdbx") || name.ends_with(".kdb") => SensitiveKind::PasswordStore,
            _ => continue,
        };
        return Some(SensitiveWarning {
            kind,
            reason: match kind {
                SensitiveKind::CredentialLocation => {
                    "A recorded path component may identify a credential or authentication location."
                }
                SensitiveKind::CredentialFile => {
                    "A recorded file or folder name may contain credentials or private configuration."
                }
                SensitiveKind::KeyMaterial => {
                    "A recorded name may contain a key or certificate, including public material."
                }
                SensitiveKind::PasswordStore => {
                    "A recorded name may contain a password or browser login database."
                }
                SensitiveKind::PrivateApplicationState => {
                    "A recorded agent-state location may contain authentication or private conversation data."
                }
            },
        });
    }
    None
}
