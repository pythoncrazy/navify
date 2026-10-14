use std::sync::mpsc;

use tokio::sync::mpsc::UnboundedSender;

use crate::app::{Message, task};

#[cfg(target_os = "linux")]
struct Hold(dbus::blocking::Connection, dbus::Path<'static>);

#[cfg(target_os = "linux")]
fn hold() -> Result<Hold, String> {
    use dbus::arg::{PropMap, Variant};
    let c = dbus::blocking::Connection::new_session().map_err(|e| e.to_string())?;
    let p = "/org/freedesktop/portal/desktop";
    let p = c.with_proxy(
        "org.freedesktop.portal.Desktop",
        p,
        std::time::Duration::from_secs(5),
    );
    let reason = Variant(Box::new("Navify plays music".to_string()) as Box<dyn dbus::arg::RefArg>);
    let opts = PropMap::from([("reason".to_string(), reason)]);
    let (h,): (dbus::Path<'static>,) = p
        .method_call(
            "org.freedesktop.portal.Inhibit",
            "Inhibit",
            ("", 4u32, opts),
        )
        .map_err(|e| e.to_string())?;
    Ok(Hold(c, h))
}

#[cfg(target_os = "linux")]
impl Drop for Hold {
    fn drop(&mut self) {
        let t = std::time::Duration::from_secs(5);
        let p = self
            .0
            .with_proxy("org.freedesktop.portal.Desktop", &self.1, t);
        let _: Result<(), _> = p.method_call("org.freedesktop.portal.Request", "Close", ());
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetThreadExecutionState(flags: u32) -> u32;
}

#[cfg(windows)]
struct Hold;

#[cfg(windows)]
fn hold() -> Result<Hold, String> {
    match unsafe { SetThreadExecutionState(0x8000_0001) } {
        0 => Err(std::io::Error::last_os_error().to_string()),
        _ => Ok(Hold),
    }
}

#[cfg(windows)]
impl Drop for Hold {
    fn drop(&mut self) {
        unsafe { SetThreadExecutionState(0x8000_0000) };
    }
}

#[cfg(target_os = "macos")]
struct Hold(std::process::Child);

#[cfg(target_os = "macos")]
fn hold() -> Result<Hold, String> {
    let pid = std::process::id().to_string();
    let c = std::process::Command::new("caffeinate")
        .args(["-i", "-w", &pid])
        .spawn();
    c.map(Hold).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
impl Drop for Hold {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn spawn(tx: UnboundedSender<Message>) -> mpsc::Sender<bool> {
    let (s, r) = mpsc::channel();
    let run = move || {
        let mut held = None;
        for on in r {
            held = match (on, held.take()) {
                (true, None) => hold()
                    .inspect_err(|e| {
                        let e = format!("Could not keep the computer awake. {e}");
                        let _ = tx.send(task(move |a| a.toast(&e)));
                    })
                    .ok(),
                (true, h) => h,
                (false, _) => None,
            };
        }
    };
    let t = std::thread::Builder::new().name("power".into()).spawn(run);
    t.expect("the system starts a thread");
    s
}
