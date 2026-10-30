//! DCL, AutoCAD's dialog language: the tiles a .dcl file defines
//! (dialogs, and tiles of its own made from AutoCAD's), as AutoLISP's
//! `load_dialog` reads them and `new_dialog` shows one of its dialogs.

use std::collections::HashMap;

/// A tile: what it is (button, edit_box, row, ...), its attributes and
/// what's in it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tile {
    pub kind: String,
    pub attrs: HashMap<String, String>,
    pub children: Vec<Tile>,
}

impl Tile {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }

    pub fn key(&self) -> Option<&str> {
        self.attr("key")
    }

    /// Its label with the `&` that marks a mnemonic taken out.
    pub fn label(&self) -> String {
        self.attr("label").unwrap_or("").replace('&', "")
    }

    pub fn is(&self, attr: &str) -> bool {
        self.attr(attr).is_some_and(|v| v.eq_ignore_ascii_case("true"))
    }
}

/// A .dcl file's definitions, by name.
#[derive(Debug, Clone, Default)]
pub struct Dcl {
    pub defs: HashMap<String, Tile>,
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    Text(String),
    Sym(char),
}

fn tokens(src: &str) -> Vec<Token> {
    let c: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < c.len() {
        match c[i] {
            w if w.is_whitespace() => i += 1,
            '/' if c.get(i + 1) == Some(&'/') => {
                while i < c.len() && c[i] != '\n' {
                    i += 1;
                }
            }
            '/' if c.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
            }
            '"' => {
                i += 1;
                let mut s = String::new();
                while i < c.len() && c[i] != '"' {
                    if c[i] == '\\' && i + 1 < c.len() {
                        i += 1;
                        s.push(match c[i] {
                            'n' => '\n',
                            't' => '\t',
                            e => e,
                        });
                    } else {
                        s.push(c[i]);
                    }
                    i += 1;
                }
                i += 1;
                out.push(Token::Text(s));
            }
            s @ ('{' | '}' | ':' | ';' | '=') => {
                out.push(Token::Sym(s));
                i += 1;
            }
            _ => {
                let start = i;
                while i < c.len() && !c[i].is_whitespace() && !"{}:;=\"".contains(c[i]) {
                    i += 1;
                }
                out.push(Token::Word(c[start..i].iter().collect()));
            }
        }
    }
    out
}

/// The tiles AutoCAD defines itself, as DCL.
const BASE: &str = r#"
errtile : text { key = "error"; }
spacer : text { label = ""; }
spacer_0 : text { label = ""; }
spacer_1 : text { label = ""; }
ok_only : column { : button { key = "accept"; label = "OK"; is_default = true; } }
ok_cancel : row { : button { key = "accept"; label = "OK"; is_default = true; }
                  : button { key = "cancel"; label = "Cancel"; is_cancel = true; } }
ok_cancel_help : row { ok_cancel; : button { key = "help"; label = "Help"; } }
ok_cancel_help_info : row { ok_cancel_help; : button { key = "info"; label = "Info..."; } }
ok_cancel_help_errtile : column { ok_cancel_help; errtile; }
default_button : button { }
retirement_button : button { }
"#;

impl Dcl {
    /// Reads a .dcl file's text.
    pub fn parse(src: &str) -> Result<Dcl, String> {
        let mut dcl = Dcl::default();
        dcl.read(&tokens(BASE))?;
        dcl.read(&tokens(src))?;
        Ok(dcl)
    }

    fn read(&mut self, t: &[Token]) -> Result<(), String> {
        let mut i = 0;
        while i < t.len() {
            match (&t[i], t.get(i + 1)) {
                // `@include "file.dcl";` and the like: left out.
                (Token::Word(w), _) if w.starts_with('@') => {
                    while i < t.len() && t[i] != Token::Sym(';') {
                        i += 1;
                    }
                    i += 1;
                }
                (Token::Word(name), Some(Token::Sym(':'))) => {
                    let name = name.clone();
                    i += 2;
                    let tile = self.tile(t, &mut i)?;
                    self.defs.insert(name, tile);
                }
                _ => return Err(format!("DCL: unexpected {:?}", t[i])),
            }
        }
        Ok(())
    }

    /// `base { attributes and tiles }` at `i`, after the colon.
    fn tile(&self, t: &[Token], i: &mut usize) -> Result<Tile, String> {
        let Some(Token::Word(base)) = t.get(*i) else { return Err("DCL: a tile's kind is missing".into()) };
        *i += 1;
        // Made from a definition: its attributes and tiles first.
        let mut tile =
            self.defs.get(base).cloned().unwrap_or_else(|| Tile { kind: base.clone(), ..Default::default() });
        if t.get(*i) != Some(&Token::Sym('{')) {
            return Err(format!("DCL: {{ expected after {base}"));
        }
        *i += 1;
        loop {
            match (t.get(*i), t.get(*i + 1)) {
                (Some(Token::Sym('}')), _) => {
                    *i += 1;
                    // An optional `;` after it.
                    if t.get(*i) == Some(&Token::Sym(';')) {
                        *i += 1;
                    }
                    return Ok(tile);
                }
                (Some(Token::Sym(':')), _) => {
                    *i += 1;
                    let child = self.tile(t, i)?;
                    tile.children.push(child);
                }
                (Some(Token::Word(name)), Some(Token::Sym('='))) => {
                    let value = match t.get(*i + 2) {
                        Some(Token::Text(s) | Token::Word(s)) => s.clone(),
                        _ => return Err(format!("DCL: {name} has no value")),
                    };
                    tile.attrs.insert(name.clone(), value);
                    *i += 3;
                    if t.get(*i) == Some(&Token::Sym(';')) {
                        *i += 1;
                    }
                }
                // A tile by name: `ok_cancel;`.
                (Some(Token::Word(name)), Some(Token::Sym(';'))) => {
                    let child = self.defs.get(name).cloned().ok_or_else(|| format!("DCL: no tile \"{name}\""))?;
                    tile.children.push(child);
                    *i += 2;
                }
                (Some(other), _) => return Err(format!("DCL: unexpected {other:?}")),
                (None, _) => return Err("DCL: } missing".into()),
            }
        }
    }

    /// The dialog named `name`.
    pub fn dialog(&self, name: &str) -> Option<&Tile> {
        self.defs.get(name).filter(|t| t.kind == "dialog")
    }
}

/// Every tile in `tile` with a key, `tile` first.
pub fn keyed(tile: &Tile) -> Vec<&Tile> {
    let mut out: Vec<&Tile> = tile.key().map(|_| tile).into_iter().collect();
    for c in &tile.children {
        out.extend(keyed(c));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialog_with_its_own_tiles() {
        let dcl = Dcl::parse(
            r#"
            // A button of our own.
            wide_button : button { width = 20; fixed_width = true; }
            hello : dialog {
                label = "Say hello";
                : edit_box { key = "name"; label = "&Name:"; edit_width = 20; value = "World"; }
                : row {
                    : toggle { key = "loud"; label = "Loud"; }
                    : wide_button { key = "go"; label = "Go"; }
                }
                /* the usual buttons */
                ok_cancel;
            }
            "#,
        )
        .unwrap();
        let d = dcl.dialog("hello").unwrap();
        assert_eq!(d.label(), "Say hello");
        let keys: Vec<&str> = keyed(d).iter().filter_map(|t| t.key()).collect();
        assert_eq!(keys, ["name", "loud", "go", "accept", "cancel"]);
        let go = keyed(d).into_iter().find(|t| t.key() == Some("go")).unwrap();
        assert_eq!((go.kind.as_str(), go.attr("width")), ("button", Some("20")), "made from wide_button");
        assert_eq!(keyed(d)[0].label(), "Name:");
        assert!(dcl.dialog("wide_button").is_none());
        assert!(Dcl::parse("x : dialog { : nothing_known { } bad }").is_err());
    }
}
