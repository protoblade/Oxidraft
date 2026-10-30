//! AutoLISP, as AutoCAD LT 2024 brought it: files loaded with APPLOAD or
//! `(load ...)`, expressions typed at the command line, `(defun c:NAME ...)`
//! making commands, `(command ...)` running Oxidraft's, getpoint and the
//! other functions that ask the user, entsel and ssget, entget, entmod and
//! entmake, getvar and setvar, and the common list, number and string
//! functions. The interpreter asks the app for everything else through
//! [`Host::call`]; in the app it runs on a thread of its own (see
//! `lisp_host`), so it can wait while the user picks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use oxidraft_commands::lisp_ask::{Answer, Ask, AskKind};
use oxidraft_geom::Point;

/// A LISP value.
#[derive(Clone, PartialEq)]
pub enum Value {
    Nil,
    T,
    Int(i64),
    Real(f64),
    Str(String),
    Sym(String),
    List(Vec<Value>),
    /// A dotted pair, `(8 . "Walls")`.
    Pair(Box<Value>, Box<Value>),
    Func(Rc<Lambda>),
    /// An object: `<Entity name: 1a>`.
    Ename(u64),
    /// A selection set; SSADD and SSDEL change it in place.
    PickSet(Rc<RefCell<Vec<u64>>>),
}

#[derive(PartialEq)]
pub struct Lambda {
    params: Vec<String>,
    locals: Vec<String>,
    body: Vec<Value>,
}

/// A value in an object's data, as DXF group codes carry them.
#[derive(Debug, Clone, PartialEq)]
pub enum Dxf {
    Int(i64),
    Real(f64),
    Str(String),
    Point([f64; 3]),
    Ename(u64),
}

/// An object's data: `(code . value)` pairs.
pub type EntData = Vec<(i32, Dxf)>;

/// What the interpreter asks of the app.
#[derive(Debug)]
pub enum Request {
    /// Typed at the command line: a command, an option, a point. `"\\"`
    /// (PAUSE) waits for the user's input instead.
    Input(String),
    /// Objects given to a command's prompt: at a "Select objects:" one
    /// they're added; at a one-object prompt it's picked at the point.
    Select(Vec<u64>, Option<[f64; 2]>),
    /// `(command)`: cancels the command running.
    Cancel,
    GetVar(String),
    SetVar(String, String),
    Print(String),
    ReadFile(String),
    /// Waits for the user; for ssget, the filter the objects must pass.
    Ask(Ask, EntData),
    EntGet(u64),
    /// entmod (an object) or entmake (none).
    EntMod(Option<u64>, EntData),
    EntDel(u64),
    EntNext(Option<u64>),
    EntLast,
    /// grread: a click, a key, or (tracking) the cursor's next place.
    GrRead(bool),
    /// ssget's modes that don't ask: X, L, P, I, W and C.
    SsGet(String, Vec<[f64; 2]>, EntData),
    /// Sent when a job ends: what it gave (the value shown, and what's
    /// typed for it at a prompt) and the `c:` commands now defined.
    Done(Result<(String, Option<String>), String>, Vec<String>),
}

#[derive(Debug)]
pub enum Reply {
    Unit,
    Var(Option<String>),
    Set(Result<(), String>),
    File(Result<String, String>),
    Answer(Answer),
    Data(Option<EntData>),
    Ename(Option<u64>),
    Enames(Vec<u64>),
    /// The program is to stop (Esc during a PAUSE, the app closing).
    Cancelled,
}

/// What a LISP program can ask of the app.
pub trait Host {
    fn call(&mut self, request: Request) -> Reply;
}

const CANCELLED: &str = "Function cancelled";

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nil => write!(f, "nil"),
            Value::T => write!(f, "T"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Real(r) => {
                let s = format!("{r:.6}");
                write!(f, "{}", s.trim_end_matches('0').trim_end_matches('.'))
            }
            Value::Str(s) => write!(f, "\"{s}\""),
            Value::Sym(s) => write!(f, "{s}"),
            Value::List(items) => {
                write!(f, "(")?;
                for (i, v) in items.iter().enumerate() {
                    write!(f, "{}{v}", if i > 0 { " " } else { "" })?;
                }
                write!(f, ")")
            }
            Value::Pair(a, b) => write!(f, "({a} . {b})"),
            Value::Func(_) => write!(f, "#<USUBR>"),
            Value::Ename(id) => write!(f, "<Entity name: {id:x}>"),
            Value::PickSet(s) => write!(f, "<Selection set: {}>", s.borrow().len()),
        }
    }
}

impl Value {
    fn truthy(&self) -> bool {
        !matches!(self, Value::Nil) && !matches!(self, Value::List(l) if l.is_empty())
    }

    fn num(&self) -> Result<f64, String> {
        match self {
            Value::Int(i) => Ok(*i as f64),
            Value::Real(r) => Ok(*r),
            v => Err(format!("bad argument type: numberp: {v}")),
        }
    }

    fn list(&self) -> Result<&[Value], String> {
        match self {
            Value::List(l) => Ok(l),
            Value::Nil => Ok(&[]),
            v => Err(format!("bad argument type: listp {v}")),
        }
    }

    fn text(&self) -> Result<&str, String> {
        match self {
            Value::Str(s) => Ok(s),
            v => Err(format!("bad argument type: stringp {v}")),
        }
    }

    fn ename(&self) -> Result<u64, String> {
        match self {
            Value::Ename(id) => Ok(*id),
            v => Err(format!("bad argument type: lentityp {v}")),
        }
    }

    fn pickset(&self) -> Result<&Rc<RefCell<Vec<u64>>>, String> {
        match self {
            Value::PickSet(s) => Ok(s),
            v => Err(format!("bad argument type: lselsetp {v}")),
        }
    }

    fn bool(b: bool) -> Value {
        if b { Value::T } else { Value::Nil }
    }

    fn from_list(items: Vec<Value>) -> Value {
        if items.is_empty() { Value::Nil } else { Value::List(items) }
    }

    /// A point: a list of two or three numbers.
    fn point(&self) -> Option<Point> {
        match self {
            Value::List(l) if (2..=3).contains(&l.len()) && l.iter().all(|v| v.num().is_ok()) => {
                Some(Point::new(l[0].num().ok()?, l[1].num().ok()?))
            }
            _ => None,
        }
    }

    fn from_point(p: Point) -> Value {
        Value::List(vec![Value::Real(p.x), Value::Real(p.y), Value::Real(0.0)])
    }

    fn car(&self) -> Result<Value, String> {
        Ok(match self {
            Value::Pair(a, _) => (**a).clone(),
            v => v.list()?.first().cloned().unwrap_or(Value::Nil),
        })
    }

    fn cdr(&self) -> Result<Value, String> {
        Ok(match self {
            Value::Pair(_, b) => (**b).clone(),
            v => {
                let l = v.list()?;
                if l.len() <= 1 { Value::Nil } else { Value::List(l[1..].to_vec()) }
            }
        })
    }

    /// `(code . value)` or `(code x y z)` as object data.
    fn to_dxf(&self) -> Option<(i32, Dxf)> {
        let code = match self.car().ok()? {
            Value::Int(c) => c as i32,
            _ => return None,
        };
        let d = match self {
            Value::Pair(_, v) => match &**v {
                Value::Int(i) => Dxf::Int(*i),
                Value::Real(r) => Dxf::Real(*r),
                Value::Str(s) => Dxf::Str(s.clone()),
                Value::Ename(id) => Dxf::Ename(*id),
                _ => return None,
            },
            Value::List(l) => {
                let n: Vec<f64> = l[1..].iter().map(Value::num).collect::<Result<_, _>>().ok()?;
                match n.as_slice() {
                    [x, y] => Dxf::Point([*x, *y, 0.0]),
                    [x, y, z] => Dxf::Point([*x, *y, *z]),
                    [v] => Dxf::Real(*v),
                    _ => return None,
                }
            }
            _ => return None,
        };
        Some((code, d))
    }

    fn from_dxf(code: i32, d: &Dxf) -> Value {
        let code = Box::new(Value::Int(code as i64));
        match d {
            Dxf::Int(i) => Value::Pair(code, Box::new(Value::Int(*i))),
            Dxf::Real(r) => Value::Pair(code, Box::new(Value::Real(*r))),
            Dxf::Str(s) => Value::Pair(code, Box::new(Value::Str(s.clone()))),
            Dxf::Ename(id) => Value::Pair(code, Box::new(Value::Ename(*id))),
            Dxf::Point([x, y, z]) => Value::List(vec![*code, Value::Real(*x), Value::Real(*y), Value::Real(*z)]),
        }
    }

    fn data(&self) -> Result<EntData, String> {
        Ok(self.list()?.iter().filter_map(Value::to_dxf).collect())
    }

    /// What's typed for this at a command's prompt: "#x,y" for a point,
    /// absolute even with dynamic input on, as AutoCAD takes LISP's points.
    pub fn as_typed(&self) -> String {
        match self.point() {
            Some(p) => format!("#{},{}", Value::Real(p.x), Value::Real(p.y)),
            None => self.as_input(),
        }
    }

    /// The text for this: "x,y" for a point.
    fn as_input(&self) -> String {
        match (self, self.point()) {
            (_, Some(p)) => format!("{},{}", Value::Real(p.x), Value::Real(p.y)),
            (Value::Str(s), _) => s.clone(),
            (Value::Nil, _) => String::new(),
            (v, _) => v.to_string(),
        }
    }
}

/// AutoLISP's wcmatch: `*`, `?`, `#` (a digit), `@` (a letter), `.` (not
/// a letter or digit), `~` (not), `[...]`, and several patterns split by
/// commas. Not case sensitive, as ssget's filters use it.
pub fn wcmatch(s: &str, pattern: &str) -> bool {
    fn one(s: &[char], p: &[char]) -> bool {
        match p.split_first() {
            None => s.is_empty(),
            Some(('*', rest)) => (0..=s.len()).any(|i| one(&s[i..], rest)),
            Some(('[', rest)) => {
                let Some(end) = rest.iter().position(|&c| c == ']') else { return false };
                let (set, after) = (&rest[..end], &rest[end + 1..]);
                let (negate, set) = match set.split_first() {
                    Some(('~', s)) => (true, s),
                    _ => (false, set),
                };
                let Some((&c, s_rest)) = s.split_first() else { return false };
                let mut hit = false;
                let mut i = 0;
                while i < set.len() {
                    if i + 2 < set.len() && set[i + 1] == '-' {
                        hit |= (set[i]..=set[i + 2]).contains(&c);
                        i += 3;
                    } else {
                        hit |= set[i] == c;
                        i += 1;
                    }
                }
                hit != negate && one(s_rest, after)
            }
            Some(('`', rest)) if !rest.is_empty() => s.first() == Some(&rest[0]) && one(&s[1..], &rest[1..]),
            Some((&pc, rest)) => {
                let Some((&c, s_rest)) = s.split_first() else { return false };
                let ok = match pc {
                    '?' => true,
                    '#' => c.is_ascii_digit(),
                    '@' => c.is_alphabetic(),
                    '.' => !c.is_alphanumeric(),
                    _ => pc == c,
                };
                ok && one(s_rest, rest)
            }
        }
    }
    let s: Vec<char> = s.to_uppercase().chars().collect();
    pattern.to_uppercase().split(',').any(|p| {
        let (negate, p) = match p.strip_prefix('~') {
            Some(rest) => (true, rest),
            None => (false, p),
        };
        let p: Vec<char> = p.chars().collect();
        one(&s, &p) != negate
    })
}

/// Whether `text` is LISP still missing a right parenthesis, so Space
/// types a space.
pub(crate) fn open_paren(text: &str) -> bool {
    let t = text.trim_start();
    if !t.starts_with('(') {
        return false;
    }
    let (mut depth, mut quoted, mut escaped) = (0i32, false, false);
    for c in t.chars() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth -= 1,
            _ => {}
        }
    }
    quoted || depth > 0
}

/// Reads the expressions in `src`.
pub fn parse(src: &str) -> Result<Vec<Value>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while let Some(v) = read(&chars, &mut i)? {
        out.push(v);
    }
    Ok(out)
}

fn skip_blank(c: &[char], i: &mut usize) {
    loop {
        while *i < c.len() && c[*i].is_whitespace() {
            *i += 1;
        }
        if *i < c.len() && c[*i] == ';' {
            while *i < c.len() && c[*i] != '\n' {
                *i += 1;
            }
            continue;
        }
        break;
    }
}

fn read(c: &[char], i: &mut usize) -> Result<Option<Value>, String> {
    skip_blank(c, i);
    if *i >= c.len() {
        return Ok(None);
    }
    match c[*i] {
        '(' => {
            *i += 1;
            let mut items = Vec::new();
            loop {
                skip_blank(c, i);
                if *i >= c.len() {
                    return Err("malformed list on input".into());
                }
                if c[*i] == ')' {
                    *i += 1;
                    break;
                }
                // A dotted pair: (a . b).
                if c[*i] == '.' && c.get(*i + 1).is_some_and(|n| n.is_whitespace()) && !items.is_empty() {
                    *i += 1;
                    let tail = read(c, i)?.ok_or("malformed dotted pair")?;
                    skip_blank(c, i);
                    if c.get(*i) != Some(&')') {
                        return Err("malformed dotted pair".into());
                    }
                    *i += 1;
                    return Ok(Some(match tail {
                        Value::List(rest) => Value::List(items.into_iter().chain(rest).collect()),
                        Value::Nil => Value::List(items),
                        atom if items.len() == 1 => Value::Pair(Box::new(items.remove(0)), Box::new(atom)),
                        atom => Value::List(items.into_iter().chain([atom]).collect()),
                    }));
                }
                items.push(read(c, i)?.ok_or("malformed list on input")?);
            }
            Ok(Some(Value::from_list(items)))
        }
        ')' => {
            *i += 1;
            Err("extra right paren on input".into())
        }
        '\'' => {
            *i += 1;
            let v = read(c, i)?.ok_or("malformed quote")?;
            Ok(Some(Value::List(vec![Value::Sym("QUOTE".into()), v])))
        }
        '"' => {
            *i += 1;
            let mut s = String::new();
            while *i < c.len() && c[*i] != '"' {
                if c[*i] == '\\' && *i + 1 < c.len() {
                    *i += 1;
                    s.push(match c[*i] {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        'e' => '\u{1b}',
                        e => e,
                    });
                } else {
                    s.push(c[*i]);
                }
                *i += 1;
            }
            *i += 1;
            Ok(Some(Value::Str(s)))
        }
        _ => {
            let start = *i;
            while *i < c.len() && !c[*i].is_whitespace() && !"()'\";".contains(c[*i]) {
                *i += 1;
            }
            let tok: String = c[start..*i].iter().collect();
            Ok(Some(if let Ok(n) = tok.parse::<i64>() {
                Value::Int(n)
            } else if let Ok(r) = tok.parse::<f64>() {
                Value::Real(r)
            } else {
                match tok.to_ascii_uppercase().as_str() {
                    "NIL" => Value::Nil,
                    "T" => Value::T,
                    s => Value::Sym(s.to_string()),
                }
            }))
        }
    }
}

/// A number from `f64`, an integer when it is one and both were.
fn number(v: f64, int: bool) -> Value {
    if int && v.fract() == 0.0 && v.abs() < 9e15 { Value::Int(v as i64) } else { Value::Real(v) }
}

fn answer_value(a: Answer) -> Result<Value, String> {
    Ok(match a {
        Answer::Point(p) => Value::from_point(p),
        Answer::Real(r) => Value::Real(r),
        Answer::Int(i) => Value::Int(i),
        Answer::Text(s) | Answer::Keyword(s) => Value::Str(s),
        Answer::Entity(id, p) => Value::List(vec![Value::Ename(id.0), Value::from_point(p)]),
        Answer::Selection(ids) if ids.is_empty() => Value::Nil,
        Answer::Selection(ids) => Value::PickSet(Rc::new(RefCell::new(ids.into_iter().map(|i| i.0).collect()))),
        Answer::Moved(p) => Value::from_point(p),
        Answer::Nothing => Value::Nil,
        Answer::Cancelled => return Err(CANCELLED.into()),
    })
}

/// A variable's value as getvar gives it: a number, a point or text.
fn var_value(v: &str) -> Value {
    if let Ok(i) = v.parse::<i64>() {
        return Value::Int(i);
    }
    if let Ok(r) = v.parse::<f64>() {
        return Value::Real(r);
    }
    let parts: Vec<Option<f64>> = v.split(',').map(|p| p.trim().parse().ok()).collect();
    if (2..=3).contains(&parts.len()) && parts.iter().all(Option::is_some) {
        let mut l: Vec<Value> = parts.into_iter().flatten().map(Value::Real).collect();
        if l.len() == 2 {
            l.push(Value::Real(0.0));
        }
        return Value::List(l);
    }
    Value::Str(v.to_string())
}

/// The interpreter: global symbols, and local scopes while functions run.
#[derive(Default)]
pub struct Lisp {
    globals: HashMap<String, Value>,
    scopes: Vec<HashMap<String, Value>>,
    depth: usize,
    /// INITGET's bits and keywords, for the next get function.
    initget: Option<(i64, Vec<String>)>,
}

impl Lisp {
    pub fn eval_str(&mut self, host: &mut dyn Host, src: &str) -> Result<Value, String> {
        let mut last = Value::Nil;
        for form in parse(src)? {
            last = self.eval(host, &form)?;
        }
        Ok(last)
    }

    /// The commands `(defun c:NAME ...)` made.
    pub fn command_names(&self) -> Vec<String> {
        self.globals
            .iter()
            .filter(|(_, v)| matches!(v, Value::Func(_)))
            .filter_map(|(k, _)| k.strip_prefix("C:").map(str::to_string))
            .collect()
    }

    /// Runs command `name` (a `c:` function).
    pub fn run_command(&mut self, host: &mut dyn Host, name: &str) -> Result<Value, String> {
        let f =
            self.globals.get(&format!("C:{}", name.trim().to_ascii_uppercase())).cloned().ok_or("no such command")?;
        self.apply(host, &f, Vec::new())
    }

    /// After an error: no function is running any more.
    pub fn reset(&mut self) {
        self.scopes.clear();
        self.depth = 0;
        self.initget = None;
    }

    fn lookup(&self, name: &str) -> Value {
        let found = self.scopes.iter().rev().find_map(|s| s.get(name)).or_else(|| self.globals.get(name)).cloned();
        match (found, name) {
            (Some(v), _) => v,
            (None, "PI") => Value::Real(std::f64::consts::PI),
            (None, "PAUSE") => Value::Str("\\".into()),
            _ => Value::Nil,
        }
    }

    fn set(&mut self, name: &str, v: Value) {
        if let Some(scope) = self.scopes.iter_mut().rev().find(|s| s.contains_key(name)) {
            scope.insert(name.to_string(), v);
        } else {
            self.globals.insert(name.to_string(), v);
        }
    }

    fn eval(&mut self, host: &mut dyn Host, v: &Value) -> Result<Value, String> {
        match v {
            Value::Sym(s) => Ok(self.lookup(s)),
            Value::List(items) => {
                let Some(head) = items.first() else { return Ok(Value::Nil) };
                let args = &items[1..];
                if let Value::Sym(name) = head
                    && let Some(r) = self.special(host, name, args)?
                {
                    return Ok(r);
                }
                let f = match head {
                    Value::Sym(name) => match self.lookup(name) {
                        f @ Value::Func(_) => f,
                        _ => Value::Sym(name.clone()),
                    },
                    other => self.eval(host, other)?,
                };
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(self.eval(host, a)?);
                }
                self.apply(host, &f, values)
            }
            other => Ok(other.clone()),
        }
    }

    fn body(&mut self, host: &mut dyn Host, forms: &[Value]) -> Result<Value, String> {
        let mut last = Value::Nil;
        for f in forms {
            last = self.eval(host, f)?;
        }
        Ok(last)
    }

    fn lambda(params: &Value, body: &[Value]) -> Result<Value, String> {
        let names: Vec<String> = params
            .list()?
            .iter()
            .map(|p| if let Value::Sym(s) = p { Ok(s.clone()) } else { Err("bad function parameter".to_string()) })
            .collect::<Result<_, _>>()?;
        let slash = names.iter().position(|n| n == "/");
        let (params, locals) = match slash {
            Some(i) => (names[..i].to_vec(), names[i + 1..].to_vec()),
            None => (names, Vec::new()),
        };
        Ok(Value::Func(Rc::new(Lambda { params, locals, body: body.to_vec() })))
    }

    /// Special forms; `None` when `name` isn't one.
    fn special(&mut self, host: &mut dyn Host, name: &str, args: &[Value]) -> Result<Option<Value>, String> {
        let r = match name {
            "QUOTE" => args.first().cloned().unwrap_or(Value::Nil),
            "SETQ" => {
                let mut last = Value::Nil;
                for pair in args.chunks(2) {
                    let Value::Sym(s) = &pair[0] else { return Err("bad argument type: symbolp".into()) };
                    last = self.eval(host, pair.get(1).unwrap_or(&Value::Nil))?;
                    self.set(s, last.clone());
                }
                last
            }
            "DEFUN" => {
                let (Some(Value::Sym(fname)), Some(params)) = (args.first(), args.get(1)) else {
                    return Err("bad DEFUN syntax".into());
                };
                let f = Self::lambda(params, &args[2..])?;
                self.globals.insert(fname.clone(), f);
                Value::Sym(fname.clone())
            }
            "LAMBDA" => Self::lambda(args.first().unwrap_or(&Value::Nil), &args[1.min(args.len())..])?,
            "FUNCTION" => match args.first() {
                Some(v) => self.eval(host, v)?,
                None => Value::Nil,
            },
            "IF" => {
                let test = self.eval(host, args.first().unwrap_or(&Value::Nil))?;
                let branch = if test.truthy() { args.get(1) } else { args.get(2) };
                match branch {
                    Some(b) => self.eval(host, b)?,
                    None => Value::Nil,
                }
            }
            "COND" => {
                for clause in args {
                    let c = clause.list()?;
                    let Some(test) = c.first() else { continue };
                    let t = self.eval(host, test)?;
                    if t.truthy() {
                        return Ok(Some(if c.len() > 1 { self.body(host, &c[1..])? } else { t }));
                    }
                }
                Value::Nil
            }
            "WHILE" => {
                let mut last = Value::Nil;
                let mut guard = 0;
                while self.eval(host, args.first().unwrap_or(&Value::Nil))?.truthy() {
                    last = self.body(host, &args[1..])?;
                    guard += 1;
                    if guard > 1_000_000 {
                        return Err("too many loops".into());
                    }
                }
                last
            }
            "REPEAT" => {
                let n = self.eval(host, args.first().unwrap_or(&Value::Int(0)))?.num()? as i64;
                let mut last = Value::Nil;
                for _ in 0..n.clamp(0, 1_000_000) {
                    last = self.body(host, &args[1..])?;
                }
                last
            }
            "PROGN" => self.body(host, args)?,
            "AND" => {
                for a in args {
                    if !self.eval(host, a)?.truthy() {
                        return Ok(Some(Value::Nil));
                    }
                }
                Value::T
            }
            "OR" => {
                for a in args {
                    if self.eval(host, a)?.truthy() {
                        return Ok(Some(Value::T));
                    }
                }
                Value::Nil
            }
            "FOREACH" => {
                let Some(Value::Sym(var)) = args.first() else { return Err("bad FOREACH syntax".into()) };
                let list = self.eval(host, args.get(1).unwrap_or(&Value::Nil))?;
                let mut last = Value::Nil;
                self.scopes.push(HashMap::new());
                let r = (|| {
                    for item in list.list()?.to_vec() {
                        self.scopes.last_mut().map(|s| s.insert(var.clone(), item));
                        last = self.body(host, &args[2..])?;
                    }
                    Ok::<_, String>(())
                })();
                self.scopes.pop();
                r?;
                last
            }
            _ => return Ok(None),
        };
        Ok(Some(r))
    }

    fn apply(&mut self, host: &mut dyn Host, f: &Value, args: Vec<Value>) -> Result<Value, String> {
        match f {
            Value::Func(l) => {
                if self.depth > 200 {
                    return Err("stack overflow".into());
                }
                let mut scope: HashMap<String, Value> = HashMap::new();
                for (i, p) in l.params.iter().enumerate() {
                    scope.insert(p.clone(), args.get(i).cloned().unwrap_or(Value::Nil));
                }
                for local in &l.locals {
                    scope.insert(local.clone(), Value::Nil);
                }
                self.scopes.push(scope);
                self.depth += 1;
                let r = self.body(host, &l.body);
                self.depth -= 1;
                self.scopes.pop();
                r
            }
            // A quoted name: a function defined with that name, or a built-in one.
            Value::Sym(name) => match self.lookup(name) {
                f @ Value::Func(_) => self.apply(host, &f, args),
                _ => self.builtin(host, name, args),
            },
            v => Err(format!("bad function: {v}")),
        }
    }

    /// getpoint and the others that wait for the user.
    fn ask(&mut self, host: &mut dyn Host, kind: AskKind, a: &[Value], filter: EntData) -> Result<Value, String> {
        let base = a.iter().find_map(Value::point);
        let prompt = a.iter().find_map(|v| if let Value::Str(s) = v { Some(s.clone()) } else { None });
        let (bits, keywords) = self.initget.take().unwrap_or((0, Vec::new()));
        let ask = Ask {
            kind,
            prompt: prompt.unwrap_or_default(),
            base,
            keywords,
            no_null: bits & 1 != 0,
            no_zero: bits & 2 != 0,
            no_negative: bits & 4 != 0,
        };
        match host.call(Request::Ask(ask, filter)) {
            Reply::Answer(a) => answer_value(a),
            _ => Err(CANCELLED.into()),
        }
    }

    fn ename_reply(r: Reply) -> Value {
        match r {
            Reply::Ename(Some(id)) => Value::Ename(id),
            _ => Value::Nil,
        }
    }

    fn builtin(&mut self, host: &mut dyn Host, name: &str, a: Vec<Value>) -> Result<Value, String> {
        let arg = |i: usize| a.get(i).cloned().unwrap_or(Value::Nil);
        let ints = a.iter().all(|v| matches!(v, Value::Int(_)));
        let nums = || a.iter().map(Value::num).collect::<Result<Vec<f64>, String>>();
        Ok(match name {
            "+" => number(nums()?.iter().sum(), ints),
            "*" => number(nums()?.iter().product(), ints),
            "-" => {
                let n = nums()?;
                match n.split_first() {
                    Some((f, [])) => number(-f, ints),
                    Some((f, rest)) => number(rest.iter().fold(*f, |x, y| x - y), ints),
                    None => Value::Int(0),
                }
            }
            "/" => {
                let n = nums()?;
                let (f, rest) = n.split_first().ok_or("too few arguments")?;
                let mut x = *f;
                for y in rest {
                    if *y == 0.0 {
                        return Err("divide by zero".into());
                    }
                    x = if ints { (x as i64 / *y as i64) as f64 } else { x / y };
                }
                number(x, ints)
            }
            "1+" => number(arg(0).num()? + 1.0, ints),
            "1-" => number(arg(0).num()? - 1.0, ints),
            "ABS" => number(arg(0).num()?.abs(), ints),
            "SQRT" => Value::Real(arg(0).num()?.sqrt()),
            "SIN" => Value::Real(arg(0).num()?.sin()),
            "COS" => Value::Real(arg(0).num()?.cos()),
            "ATAN" => match a.get(1) {
                Some(x) => Value::Real(arg(0).num()?.atan2(x.num()?)),
                None => Value::Real(arg(0).num()?.atan()),
            },
            "EXP" => Value::Real(arg(0).num()?.exp()),
            "LOG" => Value::Real(arg(0).num()?.ln()),
            "EXPT" => number(arg(0).num()?.powf(arg(1).num()?), ints),
            "MAX" => number(nums()?.into_iter().fold(f64::MIN, f64::max), ints),
            "MIN" => number(nums()?.into_iter().fold(f64::MAX, f64::min), ints),
            "FIX" => Value::Int(arg(0).num()?.trunc() as i64),
            "FLOAT" => Value::Real(arg(0).num()?),
            "REM" => number(arg(0).num()? % arg(1).num()?, ints),
            "GCD" => {
                let (mut x, mut y) = (arg(0).num()?.abs() as i64, arg(1).num()?.abs() as i64);
                while y != 0 {
                    (x, y) = (y, x % y);
                }
                Value::Int(x)
            }
            "LOGAND" => Value::Int(nums()?.iter().fold(-1, |x, y| x & *y as i64)),
            "LOGIOR" => Value::Int(nums()?.iter().fold(0, |x, y| x | *y as i64)),
            "=" => Value::bool(
                a.windows(2).all(|w| w[0] == w[1] || w[0].num().ok().zip(w[1].num().ok()).is_some_and(|(x, y)| x == y)),
            ),
            "/=" => Value::bool(
                !(a.len() == 2 && (a[0] == a[1] || a[0].num().ok() == a[1].num().ok() && a[0].num().is_ok())),
            ),
            "<" | ">" | "<=" | ">=" => {
                let n = nums()?;
                Value::bool(n.windows(2).all(|w| match name {
                    "<" => w[0] < w[1],
                    ">" => w[0] > w[1],
                    "<=" => w[0] <= w[1],
                    _ => w[0] >= w[1],
                }))
            }
            "EQUAL" => {
                let fuzz = a.get(2).map_or(Ok(0.0), Value::num)?;
                Value::bool(equal(&arg(0), &arg(1), fuzz))
            }
            "EQ" => Value::bool(arg(0) == arg(1)),
            "NOT" | "NULL" => Value::bool(!arg(0).truthy()),
            "ZEROP" => Value::bool(arg(0).num()? == 0.0),
            "MINUSP" => Value::bool(arg(0).num()? < 0.0),
            "NUMBERP" => Value::bool(arg(0).num().is_ok()),
            "LISTP" => Value::bool(matches!(arg(0), Value::List(_) | Value::Pair(..) | Value::Nil)),
            "VL-CONSP" => Value::bool(matches!(arg(0), Value::List(_) | Value::Pair(..))),
            "ATOM" => Value::bool(!matches!(arg(0), Value::List(_) | Value::Pair(..))),
            "BOUNDP" => match arg(0) {
                Value::Sym(s) => Value::bool(self.lookup(&s).truthy()),
                _ => Value::Nil,
            },
            "VL-SYMBOLP" => Value::bool(matches!(arg(0), Value::Sym(_) | Value::T)),
            "CAR" => arg(0).car()?,
            "CDR" => arg(0).cdr()?,
            "CAAR" => arg(0).car()?.car()?,
            "CDAR" => arg(0).car()?.cdr()?,
            "CADR" => arg(0).cdr()?.car()?,
            "CDDR" => arg(0).cdr()?.cdr()?,
            "CADDR" => arg(0).cdr()?.cdr()?.car()?,
            "CADAR" => arg(0).car()?.cdr()?.car()?,
            "CONS" => match arg(1) {
                Value::List(rest) => Value::List(std::iter::once(arg(0)).chain(rest).collect()),
                Value::Nil => Value::List(vec![arg(0)]),
                atom => Value::Pair(Box::new(arg(0)), Box::new(atom)),
            },
            "LIST" => Value::from_list(a.clone()),
            "APPEND" => {
                let mut l = Vec::new();
                for v in &a {
                    l.extend(v.list()?.iter().cloned());
                }
                Value::from_list(l)
            }
            "LENGTH" => Value::Int(arg(0).list()?.len() as i64),
            "NTH" => arg(1).list()?.get(arg(0).num()? as usize).cloned().unwrap_or(Value::Nil),
            "LAST" => arg(0).list()?.last().cloned().unwrap_or(Value::Nil),
            "REVERSE" => {
                let mut l = arg(0).list()?.to_vec();
                l.reverse();
                Value::from_list(l)
            }
            "MEMBER" => {
                let l = arg(1).list()?.to_vec();
                match l.iter().position(|v| *v == arg(0)) {
                    Some(i) => Value::List(l[i..].to_vec()),
                    None => Value::Nil,
                }
            }
            "VL-POSITION" => {
                arg(1).list()?.iter().position(|v| *v == arg(0)).map_or(Value::Nil, |i| Value::Int(i as i64))
            }
            "VL-REMOVE" => Value::from_list(arg(1).list()?.iter().filter(|v| **v != arg(0)).cloned().collect()),
            "SUBST" => {
                Value::from_list(arg(2).list()?.iter().map(|v| if *v == arg(1) { arg(0) } else { v.clone() }).collect())
            }
            "ASSOC" => {
                arg(1).list()?.iter().find(|p| p.car().is_ok_and(|k| k == arg(0))).cloned().unwrap_or(Value::Nil)
            }
            "MAPCAR" => {
                let f = arg(0);
                let lists: Vec<Vec<Value>> =
                    a[1..].iter().map(|l| l.list().map(<[Value]>::to_vec)).collect::<Result<_, _>>()?;
                let n = lists.iter().map(Vec::len).min().unwrap_or(0);
                let mut out = Vec::new();
                for i in 0..n {
                    out.push(self.apply(host, &f, lists.iter().map(|l| l[i].clone()).collect())?);
                }
                Value::from_list(out)
            }
            "APPLY" => {
                let args = arg(1).list()?.to_vec();
                self.apply(host, &arg(0), args)?
            }
            "VL-SORT" => {
                let (mut l, f) = (arg(0).list()?.to_vec(), arg(1));
                // An insertion sort, as the predicate may call back into LISP.
                for i in 1..l.len() {
                    let mut j = i;
                    while j > 0 && self.apply(host, &f, vec![l[j].clone(), l[j - 1].clone()])?.truthy() {
                        l.swap(j, j - 1);
                        j -= 1;
                    }
                }
                Value::from_list(l)
            }
            "EVAL" => self.eval(host, &arg(0))?,
            "SET" => match arg(0) {
                Value::Sym(s) => {
                    self.set(&s, arg(1));
                    arg(1)
                }
                v => return Err(format!("bad argument type: symbolp {v}")),
            },
            "READ" => parse(arg(0).text()?)?.into_iter().next().unwrap_or(Value::Nil),
            "STRCAT" => Value::Str(a.iter().map(|v| v.text().map(str::to_string)).collect::<Result<String, _>>()?),
            "STRLEN" => {
                Value::Int(a.iter().map(|v| v.text().map(|s| s.chars().count())).sum::<Result<usize, _>>()? as i64)
            }
            "SUBSTR" => {
                let s: Vec<char> = arg(0).text()?.chars().collect();
                let start = (arg(1).num()? as usize).saturating_sub(1).min(s.len());
                let len = a.get(2).map_or(Ok(s.len()), |n| n.num().map(|n| n as usize))?;
                Value::Str(s[start..(start + len).min(s.len())].iter().collect())
            }
            "STRCASE" => {
                let v = arg(0);
                let s = v.text()?;
                Value::Str(if arg(1).truthy() { s.to_lowercase() } else { s.to_uppercase() })
            }
            "WCMATCH" => Value::bool(wcmatch(arg(0).text()?, arg(1).text()?)),
            "VL-STRING-SEARCH" => {
                let (pattern, s) = (arg(0), arg(1));
                let start = a.get(2).map_or(Ok(0.0), Value::num)? as usize;
                let s = s.text()?;
                s.get(start..)
                    .and_then(|rest| rest.find(pattern.text().ok()?))
                    .map_or(Value::Nil, |i| Value::Int((start + i) as i64))
            }
            "VL-STRING-SUBST" => {
                let (new, old, s) = (arg(0), arg(1), arg(2));
                Value::Str(s.text()?.replacen(old.text()?, new.text()?, 1))
            }
            "VL-STRING-TRIM" | "VL-STRING-LEFT-TRIM" | "VL-STRING-RIGHT-TRIM" => {
                let (chars, s) = (arg(0), arg(1));
                let chars: Vec<char> = chars.text()?.chars().collect();
                let s = s.text()?;
                Value::Str(
                    match name {
                        "VL-STRING-TRIM" => s.trim_matches(chars.as_slice()),
                        "VL-STRING-LEFT-TRIM" => s.trim_start_matches(chars.as_slice()),
                        _ => s.trim_end_matches(chars.as_slice()),
                    }
                    .to_string(),
                )
            }
            "VL-STRING->LIST" => Value::from_list(arg(0).text()?.chars().map(|c| Value::Int(c as i64)).collect()),
            "VL-LIST->STRING" => Value::Str(
                arg(0).list()?.iter().filter_map(|v| v.num().ok().and_then(|n| char::from_u32(n as u32))).collect(),
            ),
            "VL-PRINC-TO-STRING" => Value::Str(match arg(0) {
                Value::Str(s) => s,
                v => v.to_string(),
            }),
            "VL-PRIN1-TO-STRING" => Value::Str(arg(0).to_string()),
            "VL-FILENAME-BASE" => Value::Str(
                std::path::Path::new(arg(0).text()?)
                    .file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
            ),
            "CHR" => Value::Str(char::from_u32(arg(0).num()? as u32).map_or_else(String::new, String::from)),
            "ASCII" => Value::Int(arg(0).text()?.chars().next().map_or(0, |c| c as i64)),
            "ITOA" => Value::Str((arg(0).num()? as i64).to_string()),
            "ATOI" => Value::Int(arg(0).text()?.trim().parse::<f64>().map_or(0, |v| v as i64)),
            "ATOF" | "DISTOF" => {
                let v: Option<f64> = arg(0).text()?.trim().parse().ok();
                match (name, v) {
                    ("DISTOF", None) => Value::Nil,
                    (_, v) => Value::Real(v.unwrap_or(0.0)),
                }
            }
            "ANGTOF" => arg(0).text()?.trim().parse::<f64>().map_or(Value::Nil, |d| Value::Real(d.to_radians())),
            "RTOS" => {
                let prec = a.get(2).map_or(Ok(4.0), Value::num)? as usize;
                Value::Str(format!("{:.*}", prec, arg(0).num()?))
            }
            "ANGTOS" => {
                let prec = a.get(2).map_or(Ok(0.0), Value::num)? as usize;
                Value::Str(format!("{:.*}", prec, arg(0).num()?.to_degrees()))
            }
            "DISTANCE" => {
                let (p, q) = (arg(0).point().ok_or("bad argument type: 2D/3D point")?, arg(1).point());
                Value::Real(p.distance(q.ok_or("bad argument type: 2D/3D point")?))
            }
            "ANGLE" => {
                let (p, q) = (arg(0).point(), arg(1).point());
                let (p, q) = p.zip(q).ok_or("bad argument type: 2D/3D point")?;
                Value::Real((q.y - p.y).atan2(q.x - p.x).rem_euclid(std::f64::consts::TAU))
            }
            "POLAR" => {
                let p = arg(0).point().ok_or("bad argument type: 2D/3D point")?;
                let (ang, d) = (arg(1).num()?, arg(2).num()?);
                Value::from_point(Point::new(p.x + d * ang.cos(), p.y + d * ang.sin()))
            }
            "INTERS" => {
                let pts: Vec<Point> = (0..4).filter_map(|i| arg(i).point()).collect();
                let [p1, p2, p3, p4] = pts[..] else { return Err("bad argument type: 2D/3D point".into()) };
                let on_segments = a.len() < 5 || arg(4).truthy();
                let (d1, d2) = (p2 - p1, p4 - p3);
                let den = d1.x * d2.y - d1.y * d2.x;
                if den.abs() < 1e-12 {
                    return Ok(Value::Nil);
                }
                let t = ((p3.x - p1.x) * d2.y - (p3.y - p1.y) * d2.x) / den;
                let u = ((p3.x - p1.x) * d1.y - (p3.y - p1.y) * d1.x) / den;
                if on_segments && !((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)) {
                    return Ok(Value::Nil);
                }
                Value::from_point(p1 + d1 * t)
            }
            "TRANS" => arg(0),
            "PRINC" | "PRINT" | "PRIN1" | "PROMPT" | "ALERT" => {
                // (princ) ends a function quietly.
                if a.is_empty() {
                    return Ok(Value::Sym(String::new()));
                }
                let v = arg(0);
                let shown = match (&v, name) {
                    (Value::Str(s), "PRINC" | "PROMPT" | "ALERT") => s.clone(),
                    _ => v.to_string(),
                };
                host.call(Request::Print(shown));
                v
            }
            "TERPRI" | "REDRAW" | "GRAPHSCR" | "TEXTSCR" | "VL-LOAD-COM" => Value::Nil,
            "ENTUPD" => arg(0),
            "EXIT" | "QUIT" => return Err("quit / exit abort".into()),
            "COMMAND" | "COMMAND-S" | "VL-CMDF" => {
                if a.is_empty() {
                    host.call(Request::Cancel);
                }
                for v in &a {
                    let request = match v {
                        Value::Nil => continue,
                        Value::Ename(id) => Request::Select(vec![*id], None),
                        Value::PickSet(s) => Request::Select(s.borrow().clone(), None),
                        // What entsel gives: the object and where it was picked.
                        Value::List(l) if matches!(l.first(), Some(Value::Ename(_))) => {
                            let at = l.get(1).and_then(Value::point).map(|p| [p.x, p.y]);
                            Request::Select(vec![l[0].ename()?], at)
                        }
                        v => Request::Input(v.as_typed()),
                    };
                    if let Reply::Cancelled = host.call(request) {
                        return Err(CANCELLED.into());
                    }
                }
                Value::Nil
            }
            "GETVAR" => match host.call(Request::GetVar(arg(0).text()?.to_string())) {
                Reply::Var(Some(v)) => var_value(&v),
                _ => Value::Nil,
            },
            "SETVAR" => {
                let v = arg(1);
                let text = match &v {
                    Value::Str(s) => s.clone(),
                    other => other.as_input(),
                };
                if let Reply::Set(Err(e)) = host.call(Request::SetVar(arg(0).text()?.to_string(), text)) {
                    return Err(e);
                }
                v
            }
            "LOAD" => match host.call(Request::ReadFile(arg(0).text()?.to_string())) {
                Reply::File(Ok(src)) => self.eval_str(host, &src)?,
                Reply::File(Err(e)) if a.len() < 2 => return Err(e),
                _ => arg(1),
            },
            "INITGET" => {
                let (bits, words) = match (arg(0), arg(1)) {
                    (Value::Str(w), _) => (0, w),
                    (b, Value::Str(w)) => (b.num()? as i64, w),
                    (b, _) => (b.num()? as i64, String::new()),
                };
                self.initget = Some((bits, words.split_whitespace().map(str::to_string).collect()));
                Value::Nil
            }
            "GETPOINT" => self.ask(host, AskKind::Point, &a, Vec::new())?,
            "GETCORNER" => self.ask(host, AskKind::Corner, &a, Vec::new())?,
            "GETDIST" => self.ask(host, AskKind::Dist, &a, Vec::new())?,
            "GETANGLE" | "GETORIENT" => self.ask(host, AskKind::Angle, &a, Vec::new())?,
            "GETREAL" => self.ask(host, AskKind::Real, &a, Vec::new())?,
            "GETINT" => self.ask(host, AskKind::Int, &a, Vec::new())?,
            "GETKWORD" => self.ask(host, AskKind::Kword, &a, Vec::new())?,
            "GETSTRING" => {
                let spaces = a.len() > 1 && arg(0).truthy() || !matches!(arg(0), Value::Str(_) | Value::Nil);
                self.ask(host, AskKind::Str { spaces }, &a, Vec::new())?
            }
            "ENTSEL" | "NENTSEL" => self.ask(host, AskKind::EntSel, &a, Vec::new())?,
            // (3 point) a click, (2 code) a key (13 Enter), (5 point) the
            // cursor moved, when tracking.
            "GRREAD" => match host.call(Request::GrRead(arg(0).truthy())) {
                Reply::Answer(Answer::Point(p)) => Value::List(vec![Value::Int(3), Value::from_point(p)]),
                Reply::Answer(Answer::Moved(p)) => Value::List(vec![Value::Int(5), Value::from_point(p)]),
                Reply::Answer(Answer::Text(t)) if !t.is_empty() => {
                    Value::List(vec![Value::Int(2), Value::Int(t.chars().next().map_or(13, |c| c as i64))])
                }
                Reply::Answer(Answer::Text(_) | Answer::Nothing) => Value::List(vec![Value::Int(2), Value::Int(13)]),
                _ => return Err(CANCELLED.into()),
            },
            "SSGET" => {
                let mut mode = String::new();
                let mut points = Vec::new();
                let mut filter = Vec::new();
                for v in &a {
                    match v {
                        Value::Str(s) => mode = s.trim_start_matches(['_', '+', '.']).to_uppercase(),
                        v if v.point().is_some() => points.extend(v.point().map(|p| [p.x, p.y])),
                        Value::List(_) => filter = v.data()?,
                        _ => {}
                    }
                }
                if mode.is_empty() && points.len() == 1 {
                    mode = "PT".into();
                }
                if mode.is_empty() || mode.starts_with(':') {
                    // Interactive; the filter keeps what passes.
                    self.initget = None;
                    let mut v = self.ask(host, AskKind::SsGet, &[], filter)?;
                    if let Value::PickSet(s) = &v
                        && s.borrow().is_empty()
                    {
                        v = Value::Nil;
                    }
                    v
                } else {
                    match host.call(Request::SsGet(mode, points, filter)) {
                        Reply::Enames(ids) if !ids.is_empty() => Value::PickSet(Rc::new(RefCell::new(ids))),
                        _ => Value::Nil,
                    }
                }
            }
            "SSLENGTH" => Value::Int(arg(0).pickset()?.borrow().len() as i64),
            "SSNAME" => {
                let i = arg(1).num()? as usize;
                arg(0).pickset()?.borrow().get(i).map_or(Value::Nil, |id| Value::Ename(*id))
            }
            "SSADD" => match (arg(0), arg(1)) {
                (Value::Nil, _) => Value::PickSet(Rc::new(RefCell::new(Vec::new()))),
                (e, Value::Nil) => Value::PickSet(Rc::new(RefCell::new(vec![e.ename()?]))),
                (e, set) => {
                    let id = e.ename()?;
                    {
                        let mut s = set.pickset()?.borrow_mut();
                        if !s.contains(&id) {
                            s.push(id);
                        }
                    }
                    set
                }
            },
            "SSDEL" => {
                let (id, set) = (arg(0).ename()?, arg(1));
                let removed = {
                    let mut s = set.pickset()?.borrow_mut();
                    let before = s.len();
                    s.retain(|i| *i != id);
                    s.len() < before
                };
                if removed { set } else { Value::Nil }
            }
            "SSMEMB" => {
                let id = arg(0).ename()?;
                if arg(1).pickset()?.borrow().contains(&id) { Value::Ename(id) } else { Value::Nil }
            }
            "ENTGET" => match host.call(Request::EntGet(arg(0).ename()?)) {
                Reply::Data(Some(d)) => Value::List(d.iter().map(|(c, v)| Value::from_dxf(*c, v)).collect()),
                _ => Value::Nil,
            },
            "ENTMOD" | "ENTMAKE" => {
                let data = arg(0).data()?;
                let id = match name {
                    "ENTMOD" => Some(
                        data.iter()
                            .find_map(|(c, v)| if let (-1, Dxf::Ename(id)) = (c, v) { Some(*id) } else { None })
                            .ok_or("bad entmod list")?,
                    ),
                    _ => None,
                };
                match host.call(Request::EntMod(id, data)) {
                    Reply::Ename(Some(_)) => arg(0),
                    _ => Value::Nil,
                }
            }
            "ENTDEL" => Self::ename_reply(host.call(Request::EntDel(arg(0).ename()?))),
            "ENTLAST" => Self::ename_reply(host.call(Request::EntLast)),
            "ENTNEXT" => {
                let after = match arg(0) {
                    Value::Nil => None,
                    v => Some(v.ename()?),
                };
                Self::ename_reply(host.call(Request::EntNext(after)))
            }
            "TYPE" => Value::Sym(
                match arg(0) {
                    Value::Int(_) => "INT",
                    Value::Real(_) => "REAL",
                    Value::Str(_) => "STR",
                    Value::List(_) | Value::Pair(..) => "LIST",
                    Value::Func(_) => "USUBR",
                    Value::Sym(_) | Value::T => "SYM",
                    Value::Ename(_) => "ENAME",
                    Value::PickSet(_) => "PICKSET",
                    Value::Nil => return Ok(Value::Nil),
                }
                .into(),
            ),
            "VER" => Value::Str(format!("Visual LISP (Oxidraft {})", env!("CARGO_PKG_VERSION"))),
            other => return Err(format!("no function definition: {other}")),
        })
    }
}

/// EQUAL: the same, numbers within `fuzz` of each other.
fn equal(a: &Value, b: &Value, fuzz: f64) -> bool {
    match (a, b) {
        (Value::List(x), Value::List(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| equal(p, q, fuzz)),
        (Value::Pair(a1, b1), Value::Pair(a2, b2)) => equal(a1, a2, fuzz) && equal(b1, b2, fuzz),
        _ => match (a.num(), b.num()) {
            (Ok(x), Ok(y)) => (x - y).abs() <= fuzz,
            _ => a == b,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Test {
        typed: Vec<String>,
        printed: Vec<String>,
        vars: HashMap<String, String>,
        answers: Vec<Answer>,
    }

    impl Host for Test {
        fn call(&mut self, r: Request) -> Reply {
            match r {
                Request::Input(s) => {
                    self.typed.push(s);
                    Reply::Unit
                }
                Request::Print(s) => {
                    self.printed.push(s);
                    Reply::Unit
                }
                Request::GetVar(n) => Reply::Var(self.vars.get(&n.to_ascii_uppercase()).cloned()),
                Request::SetVar(n, v) => {
                    self.vars.insert(n.to_ascii_uppercase(), v);
                    Reply::Set(Ok(()))
                }
                Request::ReadFile(_) => Reply::File(Ok("(setq loaded T)".into())),
                Request::Ask(..) => {
                    Reply::Answer(if self.answers.is_empty() { Answer::Cancelled } else { self.answers.remove(0) })
                }
                Request::EntGet(id) => Reply::Data(Some(vec![
                    (-1, Dxf::Ename(id)),
                    (0, Dxf::Str("LINE".into())),
                    (8, Dxf::Str("Walls".into())),
                    (10, Dxf::Point([1.0, 2.0, 0.0])),
                ])),
                Request::EntMod(id, data) => {
                    self.printed.push(format!("{id:?} {data:?}"));
                    Reply::Ename(Some(id.unwrap_or(99)))
                }
                _ => Reply::Unit,
            }
        }
    }

    fn run(l: &mut Lisp, h: &mut Test, src: &str) -> String {
        l.eval_str(h, src).map(|v| v.to_string()).unwrap_or_else(|e| format!("error: {e}"))
    }

    #[test]
    fn arithmetic_lists_strings_and_functions() {
        let (mut l, mut h) = (Lisp::default(), Test::default());
        assert_eq!(run(&mut l, &mut h, "(+ 1 2 (* 3 4))"), "15");
        assert_eq!(run(&mut l, &mut h, "(/ 7 2)"), "3");
        assert_eq!(run(&mut l, &mut h, "(/ 7.0 2)"), "3.5");
        assert_eq!(run(&mut l, &mut h, "(setq p '(1 2) q (list 4 6))"), "(4 6)");
        assert_eq!(run(&mut l, &mut h, "(distance p q)"), "5");
        assert_eq!(run(&mut l, &mut h, "(strcat \"a\" (itoa 12) (strcase \"bc\"))"), "\"a12BC\"");
        assert_eq!(run(&mut l, &mut h, "(defun sq (x) (* x x)) (mapcar 'sq '(1 2 3))"), "(1 4 9)");
        assert_eq!(
            run(&mut l, &mut h, "(defun f (n / acc) (setq acc 1) (repeat n (setq acc (* acc 2))) acc) (f 10)"),
            "1024"
        );
        assert_eq!(run(&mut l, &mut h, "(cond ((> 1 2) \"no\") ((= 2 2.0) \"yes\"))"), "\"yes\"");
        assert_eq!(run(&mut l, &mut h, "acc"), "nil", "a local doesn't leak");
        assert_eq!(run(&mut l, &mut h, "(* 2 pi)"), "6.283185");
        assert_eq!(run(&mut l, &mut h, "(vl-sort '(3 1 2) '<)"), "(1 2 3)");
        assert_eq!(run(&mut l, &mut h, "(inters '(0 0) '(10 10) '(0 10) '(10 0))"), "(5 5 0)");
        assert_eq!(
            run(
                &mut l,
                &mut h,
                "(list (wcmatch \"Walls-2\" \"WALL*,DOOR?\") (wcmatch \"A1\" \"@#\") (wcmatch \"x\" \"~x\"))"
            ),
            "(T T nil)"
        );
        // ; comments, load
        assert_eq!(run(&mut l, &mut h, "; nothing\n(load \"x.lsp\") loaded"), "T");
    }

    #[test]
    fn dotted_pairs_and_object_data() {
        let (mut l, mut h) = (Lisp::default(), Test::default());
        assert_eq!(
            run(&mut l, &mut h, "(setq ed '((0 . \"LINE\") (8 . \"Walls\")))"),
            "((0 . \"LINE\") (8 . \"Walls\"))"
        );
        assert_eq!(run(&mut l, &mut h, "(cdr (assoc 8 ed))"), "\"Walls\"");
        assert_eq!(run(&mut l, &mut h, "(cons 62 1)"), "(62 . 1)");
        assert_eq!(run(&mut l, &mut h, "(cons 1 '(2))"), "(1 2)");
        // entget's list, changed with subst and given to entmod.
        h.answers.push(Answer::Entity(oxidraft_model::EntityId(7), Point::new(1.0, 2.0)));
        assert_eq!(
            run(&mut l, &mut h, "(setq ed (entget (car (entsel))))"),
            "((-1 . <Entity name: 7>) (0 . \"LINE\") (8 . \"Walls\") (10 1 2 0))"
        );
        assert_eq!(run(&mut l, &mut h, "(cdr (assoc 10 ed))"), "(1 2 0)");
        run(&mut l, &mut h, "(entmod (subst (cons 8 \"Doors\") (assoc 8 ed) ed))");
        assert!(h.printed.last().unwrap().contains("Str(\"Doors\")"), "{:?}", h.printed);
    }

    #[test]
    fn commands_variables_and_asking() {
        let (mut l, mut h) = (Lisp::default(), Test::default());
        run(&mut l, &mut h, "(defun c:box (/ p) (setq p '(10 0)) (command \"_.LINE\" '(0 0) p \"\") (princ \"done\"))");
        assert_eq!(l.command_names(), ["BOX"]);
        l.run_command(&mut h, "box").unwrap();
        assert_eq!(h.typed, ["_.LINE", "#0,0", "#10,0", ""]);
        assert_eq!(h.printed, ["done"]);
        run(&mut l, &mut h, "(setvar \"FILLETRAD\" 2.5)");
        assert_eq!(run(&mut l, &mut h, "(getvar \"filletrad\")"), "2.5");
        h.answers = vec![Answer::Point(Point::new(3.0, 4.0)), Answer::Keyword("Yes".into())];
        assert_eq!(run(&mut l, &mut h, "(getpoint \"Where: \")"), "(3 4 0)");
        assert_eq!(run(&mut l, &mut h, "(initget 1 \"Yes No\") (getkword \"Sure? [Yes/No]: \")"), "\"Yes\"");
        assert_eq!(run(&mut l, &mut h, "(getpoint)"), "error: Function cancelled");
    }
}
