//! AutoLISP in the app. The interpreter runs on a thread of its own and
//! asks the app for everything it does (commands, variables, objects),
//! so getpoint, entsel, ssget and `(command ... pause ...)` can wait for
//! the user while the app goes on drawing.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use oxidraft_commands::Engine;
use oxidraft_commands::lisp_ask::{Answer, LispAsk, Slot};
use oxidraft_model::EntityId;

use crate::app::OxidraftApp;
use crate::lisp::{EntData, Host, Lisp, Reply, Request, Value};
use crate::lisp_ents::{ent_data, passes};

enum Job {
    Eval(String),
    Command(String),
}

struct Link {
    jobs: Sender<Job>,
    requests: Receiver<Request>,
    replies: Sender<Reply>,
}

/// What the interpreter waits for from the user.
enum Waiting {
    /// A get function's prompt, and ssget's filter.
    Ask(Slot, EntData),
    /// PAUSE in `(command ...)`: an input to the command, counted from this.
    Pause(usize),
}

#[derive(Default)]
pub(crate) struct LispThread {
    link: Option<Link>,
    busy: bool,
    pumping: bool,
    waiting: Option<Waiting>,
    /// The commands `(defun c:NAME ...)` made.
    commands: HashSet<String>,
    /// The job's value is the input at the prompt it was typed at.
    at_prompt: bool,
    /// The file the job loads, to say it loaded.
    loading: Option<String>,
}

impl LispThread {
    /// True while a program runs and doesn't wait for the user.
    pub(crate) fn running(&self) -> bool {
        self.busy && self.waiting.is_none()
    }
}

/// The interpreter's side: each request waits for its reply.
struct Remote {
    requests: Sender<Request>,
    replies: Receiver<Reply>,
}

impl Host for Remote {
    fn call(&mut self, r: Request) -> Reply {
        if self.requests.send(r).is_err() {
            return Reply::Cancelled;
        }
        self.replies.recv().unwrap_or(Reply::Cancelled)
    }
}

fn spawn() -> Option<Link> {
    let (jobs, job_rx) = channel::<Job>();
    let (req_tx, requests) = channel();
    let (replies, rep_rx) = channel();
    std::thread::Builder::new()
        .name("AutoLISP".into())
        .stack_size(64 << 20)
        .spawn(move || {
            let mut lisp = Lisp::default();
            let mut host = Remote { requests: req_tx, replies: rep_rx };
            while let Ok(job) = job_rx.recv() {
                let r = match job {
                    Job::Eval(src) => lisp.eval_str(&mut host, &src),
                    Job::Command(name) => lisp.run_command(&mut host, &name),
                };
                lisp.reset();
                let r = r.map(|v| (v.to_string(), (!matches!(v, Value::Nil)).then(|| v.as_typed())));
                if host.requests.send(Request::Done(r, lisp.command_names())).is_err() {
                    break;
                }
            }
        })
        .ok()?;
    Some(Link { jobs, requests, replies })
}

impl OxidraftApp {
    fn lisp_start(&mut self, job: Job, at_prompt: bool) {
        if self.lisp.link.is_none() {
            self.lisp.link = spawn();
        }
        let Some(link) = &self.lisp.link else { return self.engine.log("; error: AutoLISP couldn't start") };
        if link.jobs.send(job).is_err() {
            self.lisp.link = None;
            return self.engine.log("; error: AutoLISP stopped");
        }
        self.lisp.busy = true;
        self.lisp.at_prompt = at_prompt;
        self.lisp_pump();
    }

    /// Answers the interpreter until it's done or waits for the user.
    /// Called when a job starts, after each command-line input and each
    /// frame.
    pub(crate) fn lisp_pump(&mut self) {
        if self.lisp.pumping || !self.lisp.busy {
            return;
        }
        self.lisp.pumping = true;
        self.pump();
        self.lisp.pumping = false;
    }

    fn pump(&mut self) {
        loop {
            if self.lisp.waiting.is_some() {
                match self.lisp_answered() {
                    Some(reply) => self.lisp_reply(reply),
                    None => return,
                }
            }
            let Some(link) = &self.lisp.link else {
                self.lisp.busy = false;
                return;
            };
            match link.requests.recv_timeout(Duration::from_secs(2)) {
                Ok(Request::Done(r, commands)) => {
                    self.lisp.busy = false;
                    self.lisp.commands = commands.into_iter().collect();
                    return self.lisp_done(r);
                }
                Ok(request) => {
                    if let Some(reply) = self.lisp_request(request) {
                        self.lisp_reply(reply);
                    }
                }
                // Still working: the next frame carries on.
                Err(RecvTimeoutError::Timeout) => return,
                Err(RecvTimeoutError::Disconnected) => {
                    self.lisp = Default::default();
                    return self.engine.log("; error: AutoLISP stopped");
                }
            }
        }
    }

    fn lisp_reply(&mut self, reply: Reply) {
        if let Some(link) = &self.lisp.link {
            let _ = link.replies.send(reply);
        }
    }

    /// The user's answer to what the interpreter waits for, once given.
    fn lisp_answered(&mut self) -> Option<Reply> {
        let reply = match self.lisp.waiting.as_ref()? {
            Waiting::Ask(slot, filter) => {
                let answer = slot.borrow_mut().take()?;
                Reply::Answer(match answer {
                    Answer::Selection(ids) if !filter.is_empty() => Answer::Selection(
                        ids.into_iter().filter(|id| self.doc.entity(*id).is_some_and(|e| passes(e, filter))).collect(),
                    ),
                    a => a,
                })
            }
            Waiting::Pause(fed) if self.engine.inputs_fed() != *fed => Reply::Unit,
            Waiting::Pause(_) if !self.engine.is_active() => Reply::Cancelled,
            Waiting::Pause(_) => return None,
        };
        self.lisp.waiting = None;
        Some(reply)
    }

    /// Does what the interpreter asks. `None` when the reply waits for
    /// the user.
    fn lisp_request(&mut self, request: Request) -> Option<Reply> {
        Some(match request {
            Request::Input(s) if s == "\\" => {
                self.lisp.waiting = Some(Waiting::Pause(self.engine.inputs_fed()));
                return None;
            }
            Request::Input(s) => {
                // Enter with no command running would repeat the last one.
                if !s.is_empty() || self.engine.is_active() {
                    self.type_command(&s);
                }
                Reply::Unit
            }
            Request::Select(ids, at) => {
                let ids: Vec<EntityId> = ids.into_iter().map(EntityId).collect();
                if self.engine.is_picking() {
                    let at = at.map(|[x, y]| oxidraft_geom::Point::new(x, y)).or_else(|| {
                        let e = self.doc.entity(*ids.first()?)?;
                        e.geometry.segments().first().map(|s| s.point_at(0.5))
                    });
                    if let Some(p) = at {
                        let tolerance = self.pick_tolerance();
                        self.engine.click(p, p, false, &mut self.doc, tolerance);
                    }
                } else if self.engine.is_selecting() {
                    let mut selected = self.engine.selection().to_vec();
                    selected.extend(ids.into_iter().filter(|id| !selected.contains(id)).collect::<Vec<_>>());
                    self.engine.set_selection(selected);
                }
                Reply::Unit
            }
            Request::Cancel => {
                self.engine.cancel(&mut self.doc);
                Reply::Unit
            }
            Request::GetVar(name) => Reply::Var(self.get_var(&name)),
            Request::SetVar(name, value) => Reply::Set(self.try_set_var(&name, &value)),
            Request::Print(text) => {
                for line in text.lines() {
                    self.engine.log(line.to_string());
                }
                Reply::Unit
            }
            Request::ReadFile(name) => Reply::File(self.lisp_file(&name)),
            Request::Ask(ask, filter) => {
                let slot = Slot::default();
                self.engine.run_with("APPLOAD", "", Box::new(LispAsk::new(ask, slot.clone())), &mut self.doc);
                self.lisp.waiting = Some(Waiting::Ask(slot, filter));
                return None;
            }
            Request::EntGet(id) => Reply::Data(self.doc.entity(EntityId(id)).map(ent_data)),
            Request::EntMod(id, data) => Reply::Ename(self.ent_mod(id.map(EntityId), &data).map(|id| id.0)),
            Request::EntDel(id) => {
                self.doc.begin("ENTDEL");
                let gone = self.doc.remove(EntityId(id));
                self.doc.commit();
                Reply::Ename(gone.then_some(id))
            }
            Request::EntLast => Reply::Ename(self.doc.entities().last().map(|e| e.id.0)),
            Request::EntNext(after) => {
                let mut ids = self.doc.entities().map(|e| e.id.0);
                Reply::Ename(match after {
                    None => ids.next(),
                    Some(id) => ids.skip_while(|i| *i != id).nth(1),
                })
            }
            Request::SsGet(mode, points, filter) => Reply::Enames(self.ss_get(&mode, &points, &filter)),
            Request::Done(..) => Reply::Unit,
        })
    }

    fn lisp_done(&mut self, r: Result<(String, Option<String>), String>) {
        let loading = self.lisp.loading.take();
        match r {
            Ok(_) if loading.is_some() => {
                self.engine.log(format!("{} successfully loaded.", loading.unwrap_or_default()));
            }
            Ok((_, input)) if self.lisp.at_prompt && self.engine.is_active() => match input {
                Some(input) => self.type_command(&input),
                None => self.engine.log("nil"),
            },
            Ok((shown, _)) => {
                if !shown.is_empty() {
                    self.engine.log(shown);
                }
            }
            Err(e) => {
                self.engine.cancel(&mut self.doc);
                self.engine.log(format!("; error: {e}"));
            }
        }
    }

    /// `(load "name")`: beside the drawing first, ".lsp" if it has no
    /// extension.
    fn lisp_file(&self, name: &str) -> Result<String, String> {
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

    /// APPLOAD: loads an AutoLISP file.
    pub(crate) fn appload(&mut self) {
        let Some(file) = rfd::FileDialog::new().add_filter("AutoLISP", &["lsp"]).pick_file() else { return };
        self.load_lisp(&file);
    }

    pub fn load_lisp(&mut self, file: &Path) {
        if self.lisp.busy {
            return self.engine.log("AutoLISP is still running.");
        }
        let src = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => return self.engine.log(format!("Could not read {}: {e}", file.display())),
        };
        self.lisp.loading = Some(file.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()));
        self.lisp_start(Job::Eval(src), false);
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
        if self.lisp.busy || self.engine.accepts_spaces() {
            return false;
        }
        let active = self.engine.is_active();
        if !active {
            self.engine.log(format!("Command: {t}"));
        }
        self.lisp_start(Job::Eval(expr.to_string()), active);
        true
    }

    /// A command made with `(defun c:NAME ...)`; built-in commands come
    /// first, as in AutoCAD. Returns false if `name` isn't one.
    pub(crate) fn lisp_command(&mut self, name: &str) -> bool {
        let name = name.trim().trim_start_matches(['_', '.']).to_ascii_uppercase();
        if self.lisp.busy || self.engine.is_active() || !self.lisp.commands.contains(&name) || Engine::knows(&name) {
            return false;
        }
        self.engine.log(format!("Command: {name}"));
        self.lisp_start(Job::Command(name), false);
        true
    }
}
