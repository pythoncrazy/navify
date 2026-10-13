use std::any::Any;

use tokio::sync::mpsc::UnboundedSender;

use crate::app::Message;

const ICON: &[u8] = include_bytes!("../assets/icons/hicolor/32x32/apps/navify.png");
const MENU: [(&str, Message); 5] = [
    ("Show Navify", Message::Window(true)),
    ("Play or pause", Message::Toggle),
    ("Next track", Message::Next(true)),
    ("Previous track", Message::Prev),
    ("Quit", Message::Close),
];

#[cfg(target_os = "linux")]
pub fn spawn(tx: UnboundedSender<Message>) -> Result<Box<dyn Any>, String> {
    use dbus::arg::{PropMap, RefArg, Variant};
    use dbus::channel::{MatchingReceiver, Sender};
    use std::time::Duration;
    type Kids = Vec<Variant<Box<dyn RefArg>>>;
    type Ev = (i32, String, Variant<Box<dyn RefArg>>, u32);
    let props = |i: usize| {
        let (k, v) = if i == 0 {
            ("children-display", "submenu")
        } else {
            ("label", MENU[i - 1].0)
        };
        PropMap::from([(
            k.to_string(),
            Variant(Box::new(v.to_string()) as Box<dyn RefArg>),
        )])
    };
    let err = |e: dbus::Error| e.to_string();
    let c = dbus::blocking::Connection::new_session().map_err(err)?;
    let bus = c.with_proxy(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        Duration::from_secs(2),
    );
    let watcher = ("org.kde.StatusNotifierWatcher",);
    let (owned,): (bool,) = bus
        .method_call("org.freedesktop.DBus", "NameHasOwner", watcher)
        .map_err(err)?;
    if !owned {
        return Err("The desktop has no system tray.".into());
    }
    let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
    c.request_name(&name, false, true, false).map_err(err)?;
    let img = image::load_from_memory(ICON)
        .expect("the tray icon is a PNG")
        .into_rgba8();
    let argb: Vec<u8> = img
        .pixels()
        .flat_map(|p| [p[3], p[0], p[1], p[2]])
        .collect();
    let pixmap = vec![(img.width() as i32, img.height() as i32, argb)];
    let send = move |i: usize| {
        tx.send(MENU[i].1.clone())
            .map_err(|e| dbus::MethodErr::failed(&e))
    };
    let mut cr = dbus_crossroads::Crossroads::new();
    let s = send.clone();
    let sni = cr.register("org.kde.StatusNotifierItem", move |b| {
        let text = [
            ("Category", "ApplicationStatus"),
            ("Id", "navify"),
            ("Title", "Navify"),
        ];
        for (k, v) in text
            .into_iter()
            .chain([("Status", "Active"), ("IconName", "")])
        {
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
        b.property("Status").get(|_, _| Ok("normal".to_string()));
        b.property("TextDirection")
            .get(|_, _| Ok("ltr".to_string()));
        let args = ("parentId", "recursionDepth", "propertyNames");
        b.method(
            "GetLayout",
            args,
            ("revision", "layout"),
            move |_, _, _: (i32, i32, Vec<String>)| {
                let kid = |i: usize| {
                    Variant(Box::new((i as i32, props(i), Kids::new())) as Box<dyn RefArg>)
                };
                Ok((
                    1u32,
                    (0, props(0), (1..=MENU.len()).map(kid).collect::<Kids>()),
                ))
            },
        );
        let args = ("ids", "propertyNames");
        b.method(
            "GetGroupProperties",
            args,
            ("properties",),
            move |_, _, (ids, _): (Vec<i32>, Vec<String>)| {
                let ids = ids
                    .into_iter()
                    .filter(|i| (0..=MENU.len() as i32).contains(i));
                Ok((ids.map(|i| (i, props(i as usize))).collect::<Vec<_>>(),))
            },
        );
        let click = move |id: i32, ev: &str| match ev == "clicked" && id > 0 && id <= 5 {
            true => send(id as usize - 1),
            false => Ok(()),
        };
        let k = click.clone();
        let args = ("id", "eventId", "data", "timestamp");
        b.method("Event", args, (), move |_, _, (id, ev, _, _): Ev| {
            k(id, &ev)
        });
        b.method(
            "EventGroup",
            ("events",),
            ("idErrors",),
            move |_, _, (evs,): (Vec<Ev>,)| {
                evs.iter().try_for_each(|(id, ev, _, _)| click(*id, ev))?;
                Ok((Vec::<i32>::new(),))
            },
        );
        b.method(
            "AboutToShow",
            ("id",),
            ("needUpdate",),
            |_, _, _: (i32,)| Ok((false,)),
        );
        let out = ("updatesNeeded", "idErrors");
        b.method("AboutToShowGroup", ("ids",), out, |_, _, _: (Vec<i32>,)| {
            Ok((Vec::<i32>::new(), Vec::<i32>::new()))
        });
    });
    cr.insert("/StatusNotifierItem", &[sni], ());
    cr.insert("/Menu", &[menu], ());
    let w = "org.kde.StatusNotifierWatcher";
    let reg = dbus::Message::new_method_call(
        w,
        "/StatusNotifierWatcher",
        w,
        "RegisterStatusNotifierItem",
    );
    c.send(reg.expect("the D-Bus names are valid").append1(&name))
        .map_err(|()| "Cannot register the tray icon.".to_string())?;
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
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    let (menu, sep) = (Menu::new(), PredefinedMenuItem::separator());
    let entries = MENU.map(|(l, _)| MenuItem::new(l, true, None));
    let mut items: Vec<&dyn IsMenuItem> = entries.iter().map(|e| e as &dyn IsMenuItem).collect();
    items.insert(entries.len() - 1, &sep);
    menu.append_items(&items).map_err(|e| e.to_string())?;
    let ids = entries.map(|e| e.id().clone());
    let t = tx.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if let Some(i) = ids.iter().position(|id| *id == e.id) {
            drop(t.send(MENU[i].1.clone()));
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |e| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = e
        {
            drop(tx.send(Message::Window(true)));
        }
    }));
    let img = image::load_from_memory(ICON)
        .expect("the tray icon is a PNG")
        .into_rgba8();
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
