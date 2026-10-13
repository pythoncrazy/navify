use std::any::Any;

use tokio::sync::mpsc::UnboundedSender;

use crate::app::Message;

const ICON: &[u8] = include_bytes!("../assets/icons/hicolor/32x32/apps/navify.png");

fn items() -> [(&'static str, Message); 5] {
    [
        ("Show Navify", Message::Window(true)),
        ("Play or pause", Message::Toggle),
        ("Next track", Message::Next(true)),
        ("Previous track", Message::Prev),
        ("Quit", Message::Close),
    ]
}

#[cfg(target_os = "linux")]
pub fn spawn(tx: UnboundedSender<Message>) -> Result<Box<dyn Any>, String> {
    use std::time::Duration;

    use dbus::arg::{PropMap, RefArg, Variant};
    use dbus::blocking::Connection;
    use dbus::channel::{MatchingReceiver, Sender};
    use dbus::message::MatchRule;
    use dbus_crossroads::Crossroads;

    type Node = (i32, PropMap, Vec<Variant<Box<dyn RefArg>>>);
    type Ev = (i32, String, Variant<Box<dyn RefArg>>, u32);
    let props = |i: usize| -> PropMap {
        let mut p = PropMap::new();
        match i {
            0 => p.insert(
                "children-display".into(),
                Variant(Box::new("submenu".to_string())),
            ),
            _ => p.insert(
                "label".into(),
                Variant(Box::new(items()[i - 1].0.to_string())),
            ),
        };
        p
    };
    let c = Connection::new_session().map_err(|e| e.to_string())?;
    let (owned,): (bool,) = c
        .with_proxy(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            Duration::from_secs(2),
        )
        .method_call(
            "org.freedesktop.DBus",
            "NameHasOwner",
            ("org.kde.StatusNotifierWatcher",),
        )
        .map_err(|e| e.to_string())?;
    if !owned {
        return Err("The desktop has no system tray.".into());
    }
    let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
    c.request_name(&name, false, true, false)
        .map_err(|e| e.to_string())?;
    let image = image::load_from_memory(ICON)
        .expect("the tray icon is a PNG")
        .into_rgba8();
    let argb: Vec<u8> = image
        .pixels()
        .flat_map(|p| [p[3], p[0], p[1], p[2]])
        .collect();
    let pixmap = vec![(image.width() as i32, image.height() as i32, argb)];
    let mut cr = Crossroads::new();
    let send = move |m: Message| drop(tx.send(m));
    let (s1, s2) = (send.clone(), send.clone());
    let sni = cr.register("org.kde.StatusNotifierItem", move |b| {
        for (k, v) in [
            ("Category", "ApplicationStatus"),
            ("Id", "navify"),
            ("Title", "Navify"),
            ("Status", "Active"),
            ("IconName", ""),
        ] {
            b.property(k).get(move |_, _| Ok(v.to_string()));
        }
        b.property("IconPixmap").get(move |_, _| Ok(pixmap.clone()));
        b.property("ItemIsMenu").get(|_, _| Ok(false));
        b.property("Menu").get(|_, _| Ok(dbus::Path::from("/Menu")));
        let s = s1.clone();
        b.method("Activate", ("x", "y"), (), move |_, _, _: (i32, i32)| {
            s(Message::Window(true));
            Ok(())
        });
        let s = s1.clone();
        b.method(
            "SecondaryActivate",
            ("x", "y"),
            (),
            move |_, _, _: (i32, i32)| {
                s(Message::Toggle);
                Ok(())
            },
        );
    });
    let menu = cr.register("com.canonical.dbusmenu", move |b| {
        b.property("Version").get(|_, _| Ok(3u32));
        b.property("Status").get(|_, _| Ok("normal".to_string()));
        b.property("TextDirection")
            .get(|_, _| Ok("ltr".to_string()));
        b.method(
            "GetLayout",
            ("parentId", "recursionDepth", "propertyNames"),
            ("revision", "layout"),
            move |_, _, _: (i32, i32, Vec<String>)| {
                let kids = (1..=items().len())
                    .map(|i| {
                        Variant(Box::new((
                            i as i32,
                            props(i),
                            Vec::<Variant<Box<dyn RefArg>>>::new(),
                        )) as Box<dyn RefArg>)
                    })
                    .collect();
                Ok((1u32, (0, props(0), kids) as Node))
            },
        );
        b.method(
            "GetGroupProperties",
            ("ids", "propertyNames"),
            ("properties",),
            move |_, _, (ids, _): (Vec<i32>, Vec<String>)| {
                Ok((ids
                    .into_iter()
                    .filter(|i| (0..=items().len() as i32).contains(i))
                    .map(|i| (i, props(i as usize)))
                    .collect::<Vec<_>>(),))
            },
        );
        let s = s2.clone();
        let click = move |id: i32, ev: &str| {
            if ev == "clicked"
                && let Some((_, m)) = usize::try_from(id - 1)
                    .ok()
                    .and_then(|i| items().into_iter().nth(i))
            {
                s(m);
            }
        };
        let k = click.clone();
        b.method(
            "Event",
            ("id", "eventId", "data", "timestamp"),
            (),
            move |_, _, (id, ev, _, _): Ev| {
                k(id, &ev);
                Ok(())
            },
        );
        b.method(
            "EventGroup",
            ("events",),
            ("idErrors",),
            move |_, _, (evs,): (Vec<Ev>,)| {
                evs.iter().for_each(|(id, ev, _, _)| click(*id, ev));
                Ok((Vec::<i32>::new(),))
            },
        );
        b.method(
            "AboutToShow",
            ("id",),
            ("needUpdate",),
            |_, _, _: (i32,)| Ok((false,)),
        );
        b.method(
            "AboutToShowGroup",
            ("ids",),
            ("updatesNeeded", "idErrors"),
            |_, _, _: (Vec<i32>,)| Ok((Vec::<i32>::new(), Vec::<i32>::new())),
        );
    });
    cr.insert("/StatusNotifierItem", &[sni], ());
    cr.insert("/Menu", &[menu], ());
    let reg = dbus::Message::new_method_call(
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        "org.kde.StatusNotifierWatcher",
        "RegisterStatusNotifierItem",
    )
    .expect("the D-Bus names are valid")
    .append1(&name);
    c.send(reg)
        .map_err(|()| "Cannot register the tray icon.".to_string())?;
    std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || {
            c.start_receive(
                MatchRule::new_method_call(),
                Box::new(move |m, c| cr.handle_message(m, c).is_ok()),
            );
            while c.process(Duration::from_millis(i32::MAX as u64)).is_ok() {}
        })
        .map_err(|e| e.to_string())?;
    Ok(Box::new(()))
}

#[cfg(any(windows, target_os = "macos"))]
pub fn spawn(tx: UnboundedSender<Message>) -> Result<Box<dyn Any>, String> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let menu = Menu::new();
    let entries = items().map(|(l, _)| MenuItem::new(l, true, None));
    for (i, e) in entries.iter().enumerate() {
        if i == entries.len() - 1 {
            menu.append(&PredefinedMenuItem::separator())
                .map_err(|e| e.to_string())?;
        }
        menu.append(e).map_err(|e| e.to_string())?;
    }
    let ids = entries.map(|e| e.id().clone());
    let t = tx.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if let Some(i) = ids.iter().position(|id| *id == e.id) {
            drop(t.send(items()[i].1.clone()));
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
    let image = image::load_from_memory(ICON)
        .expect("the tray icon is a PNG")
        .into_rgba8();
    let (w, h) = image.dimensions();
    let icon = Icon::from_rgba(image.into_raw(), w, h).map_err(|e| e.to_string())?;
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
