use std::any::Any;

use tokio::sync::mpsc::UnboundedSender;

use crate::app::Message;

const MENU: [(&str, Message); 5] = [
    ("Show Navify", Message::Window(true)),
    ("Play or pause", Message::Toggle),
    ("Next track", Message::Next(true)),
    ("Previous track", Message::Prev),
    ("Quit", Message::Close),
];

#[cfg(any(target_os = "linux", windows, target_os = "macos"))]
fn icon() -> image::RgbaImage {
    let png = include_bytes!("../assets/icons/hicolor/32x32/apps/navify.png");
    let img = image::load_from_memory(png).expect("the tray icon is a PNG");
    img.into_rgba8()
}

#[cfg(target_os = "linux")]
pub fn spawn(tx: UnboundedSender<Message>) -> Result<Box<dyn Any>, String> {
    use dbus::arg::{PropMap, RefArg, Variant};
    use dbus::channel::{MatchingReceiver, Sender};
    use std::time::Duration;
    type Kids = Vec<Variant<Box<dyn RefArg>>>;
    type Ev = (i32, String, Variant<Box<dyn RefArg>>, u32);
    type Ids = Vec<i32>;
    let var = |s: &str| Variant(Box::new(s.to_string()) as Box<dyn RefArg>);
    let props = move |i: usize| match i {
        0 => PropMap::from([("children-display".into(), var("submenu"))]),
        _ => PropMap::from([("label".into(), var(MENU[i - 1].0))]),
    };
    let err = |e: dbus::Error| e.to_string();
    let c = dbus::blocking::Connection::new_session().map_err(err)?;
    let d = "org.freedesktop.DBus";
    let bus = c.with_proxy(d, "/org/freedesktop/DBus", Duration::from_secs(2));
    let watcher = ("org.kde.StatusNotifierWatcher",);
    let (owned,): (bool,) = bus.method_call(d, "NameHasOwner", watcher).map_err(err)?;
    if !owned {
        return Err("The desktop has no system tray.".into());
    }
    let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
    c.request_name(&name, false, true, false).map_err(err)?;
    let img = icon();
    let argb = img.pixels().flat_map(|p| [p[3], p[0], p[1], p[2]]);
    let pixmap = vec![(
        img.width() as i32,
        img.height() as i32,
        argb.collect::<Vec<u8>>(),
    )];
    let send = move |i: usize| tx.send(MENU[i].1.clone()).or(Ok(()));
    let mut cr = dbus_crossroads::Crossroads::new();
    let s = send.clone();
    let sni = cr.register("org.kde.StatusNotifierItem", move |b| {
        let keys = ["Category", "Id", "Title", "Status", "IconName"];
        let vals = ["ApplicationStatus", "navify", "Navify", "Active", ""];
        for (k, v) in keys.into_iter().zip(vals) {
            b.property(k).get(move |_, _| Ok(v.to_string()));
        }
        b.property("IconPixmap").get(move |_, _| Ok(pixmap.clone()));
        b.property("ItemIsMenu").get(|_, _| Ok(false));
        b.property("Menu").get(|_, _| Ok(dbus::Path::from("/Menu")));
        for (m, i) in [("Activate", 0), ("SecondaryActivate", 1)] {
            let s = s.clone();
            b.method(m, ("x", "y"), (), move |_, _, _: (i32, i32)| s(i));
        }
    });
    let menu = cr.register("com.canonical.dbusmenu", move |b| {
        b.property("Version").get(|_, _| Ok(3u32));
        for (k, v) in [("Status", "normal"), ("TextDirection", "ltr")] {
            b.property(k).get(move |_, _| Ok(v.to_string()));
        }
        let layout = move |_: &mut _, _: &mut _, _: (i32, i32, Vec<String>)| {
            let kid = |i| Variant(Box::new((i as i32, props(i), Kids::new())) as Box<dyn RefArg>);
            let kids = (1..=MENU.len()).map(kid).collect::<Kids>();
            Ok((1u32, (0, props(0), kids)))
        };
        let group = move |_: &mut _, _: &mut _, (ids, _): (Ids, Vec<String>)| {
            let ids = ids.into_iter().filter(|i| (0..=5).contains(i));
            Ok((ids.map(|i| (i, props(i as usize))).collect::<Vec<_>>(),))
        };
        let click = move |id: i32, ev: &str| match ev == "clicked" && id > 0 && id <= 5 {
            true => send(id as usize - 1),
            false => Ok(()),
        };
        let k = click.clone();
        let events = move |_: &mut _, _: &mut _, (evs,): (Vec<Ev>,)| {
            evs.iter().try_for_each(|(id, ev, _, _)| click(*id, ev))?;
            Ok((Ids::new(),))
        };
        let none = |_: &mut _, _: &mut _, _: (Ids,)| Ok((Ids::new(), Ids::new()));
        let a = ("parentId", "recursionDepth", "propertyNames");
        b.method("GetLayout", a, ("revision", "layout"), layout);
        let a = ("ids", "propertyNames");
        b.method("GetGroupProperties", a, ("properties",), group);
        let a = ("id", "eventId", "data", "timestamp");
        b.method("Event", a, (), move |_, _, (id, e, _, _): Ev| k(id, &e));
        b.method("EventGroup", ("events",), ("idErrors",), events);
        let (i, n, o) = (("id",), ("needUpdate",), ("updatesNeeded", "idErrors"));
        b.method("AboutToShow", i, n, |_, _, _: (i32,)| Ok((false,)));
        b.method("AboutToShowGroup", ("ids",), o, none);
    });
    cr.insert("/StatusNotifierItem", &[sni], ());
    cr.insert("/Menu", &[menu], ());
    let (w, p) = ("org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher");
    let reg = move |c: &dbus::blocking::Connection| {
        let m = dbus::Message::new_method_call(w, p, w, "RegisterStatusNotifierItem");
        c.send(m.expect("the D-Bus names are valid").append1(&name))
    };
    reg(&c).map_err(|()| "Cannot register the tray icon.".to_string())?;
    let owner = dbus::message::MatchRule::new_signal(d, "NameOwnerChanged").with_sender(d);
    let again = move |(who, _, new): (String, String, String), c: &_, _: &_| {
        let _ = (who == w && !new.is_empty()).then(|| reg(c));
        true
    };
    c.add_match(owner, again).map_err(err)?;
    let rule = dbus::message::MatchRule::new_method_call();
    c.start_receive(rule, Box::new(move |m, c| cr.handle_message(m, c).is_ok()));
    std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || while c.process(Duration::from_millis(i32::MAX as u64)).is_ok() {})
        .map_err(|e| e.to_string())?;
    Ok(Box::new(()))
}

#[cfg(any(windows, target_os = "macos"))]
pub fn spawn(tx: UnboundedSender<Message>) -> Result<Box<dyn Any>, String> {
    use tray_icon::menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState::Up, TrayIconBuilder, TrayIconEvent};
    let (menu, sep) = (Menu::new(), PredefinedMenuItem::separator());
    let entries = MENU.map(|(l, _)| MenuItem::new(l, true, None));
    let [a, b, c, d, e] = &entries;
    let items: [&dyn IsMenuItem; 6] = [a, b, c, d, &sep, e];
    menu.append_items(&items).map_err(|e| e.to_string())?;
    let (ids, t) = (entries.map(|e| e.id().clone()), tx.clone());
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if let Some(i) = ids.iter().position(|id| *id == e.id) {
            drop(t.send(MENU[i].1.clone()));
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |e| {
        if matches!(
            e,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: Up,
                ..
            }
        ) {
            drop(tx.send(Message::Window(true)));
        }
    }));
    let img = icon();
    let (w, h) = img.dimensions();
    let icon = Icon::from_rgba(img.into_raw(), w, h).map_err(|e| e.to_string())?;
    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Navify")
        .with_icon(icon)
        .with_menu_on_left_click(cfg!(target_os = "macos"))
        .build()
        .map(|t| Box::new(t) as Box<dyn Any>)
        .map_err(|e| e.to_string())
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
pub fn spawn(_: UnboundedSender<Message>) -> Result<Box<dyn Any>, String> {
    Err("This system has no tray support.".into())
}
