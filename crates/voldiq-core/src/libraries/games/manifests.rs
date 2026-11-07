use super::{Install, Record};
use crate::{NodeId, libraries::Provider};
use flate2::read::GzDecoder;
use serde::Deserialize;
use std::io::Read;

pub(super) const FILE_LIMIT: usize = 256 * 1024;

pub(super) fn steam(bytes: &[u8], parent: NodeId, filename_id: &str) -> Option<Record> {
    let text = std::str::from_utf8(bytes)
        .ok()?
        .trim_start_matches('\u{feff}');
    let mut tokens = Kv(text);
    if !matches!(tokens.next()?, Token::Text(root) if root.eq_ignore_ascii_case("AppState"))
        || !matches!(tokens.next()?, Token::Open)
    {
        return None;
    }
    let (mut id, mut folder, mut name) = (None, None, None);
    loop {
        let key = match tokens.next()? {
            Token::Close => break,
            Token::Text(key) => key,
            Token::Open => return None,
        };
        let field = match key.to_ascii_lowercase().as_str() {
            "appid" => Some(&mut id),
            "installdir" => Some(&mut folder),
            "name" => Some(&mut name),
            _ => None,
        };
        match (tokens.next()?, field) {
            (Token::Text(value), Some(field)) if field.is_none() => *field = Some(value),
            (Token::Text(_), None) => {}
            (Token::Open, None) => {
                let mut depth = 1;
                while depth != 0 {
                    match tokens.next()? {
                        Token::Open => depth += 1,
                        Token::Close => depth -= 1,
                        Token::Text(_) => {}
                    }
                    if depth > 32 {
                        return None;
                    }
                }
            }
            _ => return None,
        }
    }
    let id = id.filter(|id| id == filename_id && number(id))?;
    let folder = folder.filter(|name| component(name))?;
    Some(Record {
        provider: Provider::Steam,
        title: title(name.as_deref(), &folder),
        product: id,
        main: None,
        install: Install::Steam {
            steamapps: parent,
            folder,
        },
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Epic {
    app_name: String,
    display_name: Option<String>,
    install_location: String,
    main_game_app_name: Option<String>,
    #[serde(default, rename = "bIsIncompleteInstall")]
    incomplete: bool,
}

pub(super) fn epic(bytes: &[u8]) -> Option<Record> {
    let data: Epic = json(bytes)?;
    if data.incomplete || !product(&data.app_name) || data.install_location.len() > 32768 {
        return None;
    }
    let main = match data.main_game_app_name {
        Some(id) if !id.is_empty() && id != data.app_name => {
            Some(id).filter(|value| product(value))
        }
        _ => None,
    };
    Some(Record {
        provider: Provider::Epic,
        title: title(data.display_name.as_deref(), &data.app_name),
        product: data.app_name,
        main,
        install: Install::Absolute(data.install_location),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Gog {
    game_id: NumericId,
    root_game_id: Option<NumericId>,
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum NumericId {
    Number(u64),
    Text(String),
}

impl NumericId {
    fn text(self) -> Option<String> {
        match self {
            Self::Number(value) if value != 0 => Some(value.to_string()),
            Self::Text(value) if number(&value) => Some(value),
            _ => None,
        }
    }
}

pub(super) fn gog(bytes: &[u8], folder: NodeId, filename_id: &str) -> Option<Record> {
    let data: Gog = json(bytes)?;
    let product = data.game_id.text().filter(|value| value == filename_id)?;
    let main = match data.root_game_id {
        Some(id) => Some(id.text()?).filter(|id| id != &product),
        None => None,
    };
    Some(Record {
        provider: Provider::Gog,
        title: title(data.name.as_deref(), &product),
        product,
        main,
        install: Install::Indexed(folder),
    })
}

#[derive(Deserialize)]
struct ItchReceipt {
    game: ItchGame,
}

#[derive(Deserialize)]
struct ItchGame {
    id: NumericId,
    title: Option<String>,
}

pub(super) fn itch(bytes: &[u8], folder: NodeId) -> Option<Record> {
    let mut text = Vec::new();
    GzDecoder::new(bytes)
        .take(FILE_LIMIT as u64 + 1)
        .read_to_end(&mut text)
        .ok()?;
    if text.len() > FILE_LIMIT {
        return None;
    }
    let receipt: ItchReceipt = json(&text)?;
    let product = receipt.game.id.text()?;
    Some(Record {
        provider: Provider::Itch,
        title: title(receipt.game.title.as_deref(), &product),
        product,
        main: None,
        install: Install::Indexed(folder),
    })
}

fn json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    serde_json::from_slice(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes)).ok()
}

pub(super) fn number(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.bytes().any(|byte| byte != b'0')
}

fn product(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn component(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 1024
        && !name.ends_with(['.', ' '])
        && !name.contains(['\\', '/', ':'])
        && !name.chars().any(char::is_control)
}

fn title(value: Option<&str>, fallback: &str) -> String {
    let name: String = value
        .unwrap_or(fallback)
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(160)
        .collect();
    if name.is_empty() {
        fallback.to_owned()
    } else {
        name
    }
}

enum Token {
    Open,
    Close,
    Text(String),
}

struct Kv<'a>(&'a str);

impl Kv<'_> {
    fn next(&mut self) -> Option<Token> {
        self.0 = self.0.trim_start();
        while self.0.starts_with("//") {
            self.0 = self.0.split_once('\n')?.1.trim_start();
        }
        let token = match self.0.as_bytes().first()? {
            b'{' => Token::Open,
            b'}' => Token::Close,
            b'"' => {
                let mut value = String::new();
                let mut chars = self.0[1..].char_indices();
                while let Some((index, ch)) = chars.next() {
                    match ch {
                        '"' => {
                            self.0 = &self.0[index + 2..];
                            return Some(Token::Text(value));
                        }
                        '\\' => {
                            let (_, next) = chars.next()?;
                            if !matches!(next, '"' | '\\') {
                                value.push('\\');
                            }
                            value.push(next);
                        }
                        _ => value.push(ch),
                    }
                }
                return None;
            }
            _ => {
                let end = self
                    .0
                    .find(|ch: char| ch.is_whitespace() || matches!(ch, '{' | '}' | '"'))
                    .unwrap_or(self.0.len());
                let (value, tail) = self.0.split_at(end);
                self.0 = tail;
                return Some(Token::Text(value.to_owned()));
            }
        };
        self.0 = &self.0[1..];
        Some(token)
    }
}
