//! AutoLISP, as AutoCAD LT 2024 brought it: files loaded with APPLOAD or
//! `(load ...)`, expressions typed at the command line, `(defun c:NAME ...)`
//! making commands, `(command ...)` running Oxidraft's, `getvar` and
//! `setvar`, and the common list, number and string functions. Functions
//! that wait for picks or typing (getpoint and the like) aren't here.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use oxidraft_commands::Engine;

use crate::app::OxidraftApp;

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
    Func(Rc<Lambda>),
}

#[derive(PartialEq)]
pub struct Lambda {
    params: Vec<String>,
    locals: Vec<String>,
    body: Vec<Value>,
}

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
            Value::Func(_) => write!(f, "#<USUBR>"),
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

    fn bool(b: bool) -> Value {
        if b { Value::T } else { Value::Nil }
    }

    /// What's typed for this at a command's prompt: "#x,y" for a point,
    /// absolute even with dynamic input on, as AutoCAD takes LISP's points.
    pub(crate) fn as_typed(&self) -> String {
        match self {
            Value::List(_) if !self.as_input().starts_with('(') => format!("#{}", self.as_input()),
            _ => self.as_input(),
        }
    }

    /// The text for this: "x,y" for a point.
    fn as_input(&self) -> String {
        match self {
            Value::List(l) if l.iter().all(|v| v.num().is_ok()) && (2..=3).contains(&l.len()) => {
                l.iter().take(2).map(|v| v.to_string()).collect::<Vec<_>>().join(",")
            }
            Value::Str(s) => s.clone(),
            Value::Nil => String::new(),
            v => v.to_string(),
        }
    }
}

/// What a LISP program can ask of the app.
pub trait Host {
    /// Types `input` at the command line (a command, an option, a point).
    fn input(&mut self, input: &str);
    fn getvar(&mut self, name: &str) -> Option<String>;
    fn setvar(&mut self, name: &str, value: &str) -> Result<(), String>;
    fn print(&mut self, text: &str);
    fn read_file(&mut self, name: &str) -> Result<String, String>;
}

/// The interpreter: global symbols, and local scopes while functions run.
#[derive(Default)]
pub struct Lisp {
    globals: HashMap<String, Value>,
    scopes: Vec<HashMap<String, Value>>,
    depth: usize,
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

fn read(c: &[char], i: &mut usize) -> Result<Option<Value>, String> {
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
    if *i >= c.len() {
        return Ok(None);
    }
    match c[*i] {
        '(' => {
            *i += 1;
            let mut items = Vec::new();
            loop {
                while *i < c.len() && c[*i].is_whitespace() {
                    *i += 1;
                }
                if *i >= c.len() {
                    return Err("malformed list on input".into());
                }
                if c[*i] == ')' {
                    *i += 1;
                    break;
                }
                // A dotted pair's dot.
                if c[*i] == '.' && c.get(*i + 1).is_some_and(|n| n.is_whitespace()) {
                    *i += 1;
                    continue;
                }
                items.push(read(c, i)?.ok_or("malformed list on input")?);
            }
            Ok(Some(if items.is_empty() { Value::Nil } else { Value::List(items) }))
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

impl Lisp {
    pub fn eval_str(&mut self, host: &mut dyn Host, src: &str) -> Result<Value, String> {
        let mut last = Value::Nil;
        for form in parse(src)? {
            last = self.eval(host, &form)?;
        }
        Ok(last)
    }

    /// Whether `(defun c:NAME ...)` made command `name`.
    pub fn has_command(&self, name: &str) -> bool {
        matches!(self.globals.get(&format!("C:{}", name.trim().to_ascii_uppercase())), Some(Value::Func(_)))
    }

    /// Runs command `name` (a `c:` function).
    pub fn run_command(&mut self, host: &mut dyn Host, name: &str) -> Result<Value, String> {
        let f =
            self.globals.get(&format!("C:{}", name.trim().to_ascii_uppercase())).cloned().ok_or("no such command")?;
        self.apply(host, &f, Vec::new())
    }

    fn lookup(&self, name: &str) -> Value {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.get(name))
            .or_else(|| self.globals.get(name))
            .cloned()
            .unwrap_or(Value::Nil)
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
                        Value::Nil => Value::Sym(name.clone()),
                        f => f,
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
                for item in list.list()?.to_vec() {
                    self.scopes.last_mut().map(|s| s.insert(var.clone(), item));
                    last = self.body(host, &args[2..])?;
                }
                self.scopes.pop();
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
            "EXPT" => number(arg(0).num()?.powf(arg(1).num()?), ints),
            "MAX" => number(nums()?.into_iter().fold(f64::MIN, f64::max), ints),
            "MIN" => number(nums()?.into_iter().fold(f64::MAX, f64::min), ints),
            "FIX" => Value::Int(arg(0).num()?.trunc() as i64),
            "FLOAT" => Value::Real(arg(0).num()?),
            "REM" => number(arg(0).num()? % arg(1).num()?, ints),
            "PI" => Value::Real(std::f64::consts::PI),
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
            "EQUAL" | "EQ" => Value::bool(arg(0) == arg(1)),
            "NOT" | "NULL" => Value::bool(!arg(0).truthy()),
            "ZEROP" => Value::bool(arg(0).num()? == 0.0),
            "MINUSP" => Value::bool(arg(0).num()? < 0.0),
            "NUMBERP" => Value::bool(arg(0).num().is_ok()),
            "LISTP" => Value::bool(matches!(arg(0), Value::List(_) | Value::Nil)),
            "CAR" => arg(0).list()?.first().cloned().unwrap_or(Value::Nil),
            "CDR" => {
                let l = arg(0).list()?.to_vec();
                if l.len() <= 1 { Value::Nil } else { Value::List(l[1..].to_vec()) }
            }
            "CADR" => arg(0).list()?.get(1).cloned().unwrap_or(Value::Nil),
            "CADDR" => arg(0).list()?.get(2).cloned().unwrap_or(Value::Nil),
            "CONS" => {
                let mut l = vec![arg(0)];
                match arg(1) {
                    Value::List(rest) => l.extend(rest),
                    Value::Nil => {}
                    other => l.push(other),
                }
                Value::List(l)
            }
            "LIST" => {
                if a.is_empty() {
                    Value::Nil
                } else {
                    Value::List(a.clone())
                }
            }
            "APPEND" => {
                let mut l = Vec::new();
                for v in &a {
                    l.extend(v.list()?.iter().cloned());
                }
                if l.is_empty() { Value::Nil } else { Value::List(l) }
            }
            "LENGTH" => Value::Int(arg(0).list()?.len() as i64),
            "NTH" => arg(1).list()?.get(arg(0).num()? as usize).cloned().unwrap_or(Value::Nil),
            "LAST" => arg(0).list()?.last().cloned().unwrap_or(Value::Nil),
            "REVERSE" => {
                let mut l = arg(0).list()?.to_vec();
                l.reverse();
                if l.is_empty() { Value::Nil } else { Value::List(l) }
            }
            "MEMBER" => {
                let l = arg(1).list()?.to_vec();
                match l.iter().position(|v| *v == arg(0)) {
                    Some(i) => Value::List(l[i..].to_vec()),
                    None => Value::Nil,
                }
            }
            "ASSOC" => arg(1)
                .list()?
                .iter()
                .find(|p| p.list().is_ok_and(|l| l.first() == Some(&arg(0))))
                .cloned()
                .unwrap_or(Value::Nil),
            "MAPCAR" => {
                let f = arg(0);
                let lists: Vec<Vec<Value>> =
                    a[1..].iter().map(|l| l.list().map(<[Value]>::to_vec)).collect::<Result<_, _>>()?;
                let n = lists.iter().map(Vec::len).min().unwrap_or(0);
                let mut out = Vec::new();
                for i in 0..n {
                    out.push(self.apply(host, &f, lists.iter().map(|l| l[i].clone()).collect())?);
                }
                if out.is_empty() { Value::Nil } else { Value::List(out) }
            }
            "APPLY" => {
                let args = arg(1).list()?.to_vec();
                self.apply(host, &arg(0), args)?
            }
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
            "ITOA" => Value::Str((arg(0).num()? as i64).to_string()),
            "ATOI" => Value::Int(arg(0).text()?.trim().parse::<f64>().map_or(0, |v| v as i64)),
            "ATOF" => Value::Real(arg(0).text()?.trim().parse().unwrap_or(0.0)),
            "RTOS" => {
                let prec = a.get(2).map_or(Ok(4.0), Value::num)? as usize;
                Value::Str(format!("{:.*}", prec, arg(0).num()?))
            }
            "ANGTOS" => Value::Str(format!("{:.0}", arg(0).num()?.to_degrees())),
            "DISTANCE" => {
                let (p, q) = (arg(0), arg(1));
                let (p, q) = (p.list()?, q.list()?);
                let d = |i: usize| {
                    q.get(i).map_or(Ok(0.0), Value::num).and_then(|b| Ok(b - p.get(i).map_or(Ok(0.0), Value::num)?))
                };
                Value::Real((d(0)?.powi(2) + d(1)?.powi(2)).sqrt())
            }
            "ANGLE" => {
                let (p, q) = (arg(0), arg(1));
                let (p, q) = (p.list()?, q.list()?);
                Value::Real(
                    (q[1].num()? - p[1].num()?).atan2(q[0].num()? - p[0].num()?).rem_euclid(std::f64::consts::TAU),
                )
            }
            "POLAR" => {
                let p = arg(0);
                let p = p.list()?;
                let (ang, d) = (arg(1).num()?, arg(2).num()?);
                Value::List(vec![Value::Real(p[0].num()? + d * ang.cos()), Value::Real(p[1].num()? + d * ang.sin())])
            }
            "PRINC" | "PRINT" | "PRIN1" | "PROMPT" => {
                let v = arg(0);
                let shown = match (&v, name) {
                    (Value::Str(s), "PRINC" | "PROMPT") => s.clone(),
                    _ => v.to_string(),
                };
                // (princ) ends a function quietly.
                if a.is_empty() {
                    return Ok(Value::Sym(String::new()));
                }
                host.print(&shown);
                v
            }
            "TERPRI" => Value::Nil,
            "COMMAND" | "COMMAND-S" => {
                for v in &a {
                    host.input(&v.as_typed());
                }
                Value::Nil
            }
            "GETVAR" => host.getvar(arg(0).text()?).map_or(Value::Nil, |v| match v.parse::<i64>() {
                Ok(i) => Value::Int(i),
                Err(_) => v.parse::<f64>().map_or(Value::Str(v.clone()), Value::Real),
            }),
            "SETVAR" => {
                let v = arg(1);
                let text = match &v {
                    Value::Str(s) => s.clone(),
                    other => other.as_input(),
                };
                host.setvar(arg(0).text()?, &text)?;
                v
            }
            "LOAD" => {
                let src = host.read_file(arg(0).text()?)?;
                self.eval_str(host, &src)?
            }
            "TYPE" => Value::Sym(
                match arg(0) {
                    Value::Int(_) => "INT",
                    Value::Real(_) => "REAL",
                    Value::Str(_) => "STR",
                    Value::List(_) => "LIST",
                    Value::Func(_) => "USUBR",
                    Value::Sym(_) => "SYM",
                    Value::T => "SYM",
                    Value::Nil => return Ok(Value::Nil),
                }
                .into(),
            ),
            "VER" => Value::Str(format!("Visual LISP (Oxidraft {})", env!("CARGO_PKG_VERSION"))),
            "GETPOINT" | "GETREAL" | "GETINT" | "GETSTRING" | "GETDIST" | "GETANGLE" | "GETKWORD" | "ENTSEL"
            | "SSGET" => {
                return Err(format!("{name} waits for input, which Oxidraft's LISP can't yet"));
            }
            other => return Err(format!("no function definition: {other}")),
        })
    }
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

impl Host for OxidraftApp {
    fn input(&mut self, input: &str) {
        // Enter with no command running would repeat the last one.
        if input.is_empty() && !self.engine.is_active() {
            return;
        }
        self.type_command(input);
    }

    fn getvar(&mut self, name: &str) -> Option<String> {
        self.get_var(name)
    }

    fn setvar(&mut self, name: &str, value: &str) -> Result<(), String> {
        self.try_set_var(name, value)
    }

    fn print(&mut self, text: &str) {
        for line in text.lines() {
            self.engine.log(line.to_string());
        }
    }

    fn read_file(&mut self, name: &str) -> Result<String, String> {
        let mut path = PathBuf::from(name);
        if path.extension().is_none() {
            path.set_extension("lsp");
        }
        if path.is_relative()
            && let Some(dir) = self.path.as_ref().and_then(|p| p.parent())
            && dir.join(&path).exists()
        {
            path = dir.join(&path);
        }
        std::fs::read_to_string(&path).map_err(|e| format!("LOAD failed: \"{}\": {e}", path.display()))
    }
}

impl OxidraftApp {
    /// Runs `f` with the interpreter, which is out of the app meanwhile.
    fn with_lisp<T>(&mut self, f: impl FnOnce(&mut Lisp, &mut Self) -> T) -> T {
        let mut lisp = std::mem::take(&mut self.lisp);
        let r = f(&mut lisp, self);
        self.lisp = lisp;
        r
    }

    fn lisp_error(&mut self, e: String) {
        self.engine.cancel(&mut self.doc);
        self.engine.log(format!("; error: {e}"));
    }

    /// APPLOAD: loads an AutoLISP file.
    pub(crate) fn appload(&mut self) {
        let Some(file) = rfd::FileDialog::new().add_filter("AutoLISP", &["lsp"]).pick_file() else { return };
        self.load_lisp(&file);
    }

    pub fn load_lisp(&mut self, file: &Path) {
        let name = file.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let src = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => return self.engine.log(format!("Could not read {}: {e}", file.display())),
        };
        match self.with_lisp(|l, app| l.eval_str(app, &src)) {
            Ok(_) => self.engine.log(format!("{name} successfully loaded.")),
            Err(e) => self.lisp_error(e),
        }
    }

    /// LISP on the command line: an expression, or `!name` for a
    /// variable's value. At a command's prompt the value is the input.
    /// Returns false for anything else.
    pub(crate) fn lisp_typed(&mut self, text: &str) -> bool {
        let t = text.trim();
        let expr = match t.strip_prefix('!') {
            _ if t.starts_with('(') => t,
            Some(name) if !name.is_empty() => name,
            _ => return false,
        };
        if self.engine.accepts_spaces() {
            return false;
        }
        let active = self.engine.is_active();
        if !active {
            self.engine.log(format!("Command: {t}"));
        }
        match self.with_lisp(|l, app| l.eval_str(app, expr)) {
            Ok(Value::Nil) if active => self.engine.log("nil"),
            Ok(v) if active && self.engine.is_active() => self.type_command(&v.as_typed()),
            Ok(v) => {
                let shown = v.to_string();
                if !shown.is_empty() {
                    self.engine.log(shown);
                }
            }
            Err(e) => self.lisp_error(e),
        }
        true
    }

    /// A command made with `(defun c:NAME ...)`; built-in commands come
    /// first, as in AutoCAD. Returns false if `name` isn't one.
    pub(crate) fn lisp_command(&mut self, name: &str) -> bool {
        let name = name.trim().trim_start_matches(['_', '.']);
        if self.engine.is_active() || !self.lisp.has_command(name) || Engine::knows(name) {
            return false;
        }
        self.engine.log(format!("Command: {}", name.to_uppercase()));
        match self.with_lisp(|l, app| l.run_command(app, name)) {
            Ok(v) => {
                let shown = v.to_string();
                if !shown.is_empty() {
                    self.engine.log(shown);
                }
            }
            Err(e) => self.lisp_error(e),
        }
        true
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
    }

    impl Host for Test {
        fn input(&mut self, input: &str) {
            self.typed.push(input.into());
        }
        fn getvar(&mut self, name: &str) -> Option<String> {
            self.vars.get(&name.to_ascii_uppercase()).cloned()
        }
        fn setvar(&mut self, name: &str, value: &str) -> Result<(), String> {
            self.vars.insert(name.to_ascii_uppercase(), value.into());
            Ok(())
        }
        fn print(&mut self, text: &str) {
            self.printed.push(text.into());
        }
        fn read_file(&mut self, _: &str) -> Result<String, String> {
            Ok("(setq loaded T)".into())
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
        assert_eq!(run(&mut l, &mut h, "(assoc 8 '((0 . \"LINE\") (8 . \"Walls\")))"), "(8 \"Walls\")");
        assert_eq!(run(&mut l, &mut h, "acc"), "nil", "a local doesn't leak");
        assert!(run(&mut l, &mut h, "(getpoint)").starts_with("error"));
        // ; comments, load
        assert_eq!(run(&mut l, &mut h, "; nothing\n(load \"x.lsp\") loaded"), "T");
    }

    #[test]
    fn commands_and_variables() {
        let (mut l, mut h) = (Lisp::default(), Test::default());
        run(&mut l, &mut h, "(defun c:box (/ p) (setq p '(10 0)) (command \"_.LINE\" '(0 0) p \"\") (princ \"done\"))");
        assert!(l.has_command("BOX"));
        l.run_command(&mut h, "box").unwrap();
        assert_eq!(h.typed, ["_.LINE", "#0,0", "#10,0", ""]);
        assert_eq!(h.printed, ["done"]);
        run(&mut l, &mut h, "(setvar \"FILLETRAD\" 2.5)");
        assert_eq!(run(&mut l, &mut h, "(getvar \"filletrad\")"), "2.5");
    }
}
