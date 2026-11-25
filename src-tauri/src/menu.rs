//! The menu-bar item and its native menu (macOS).
//!
//! This is a real `NSMenu` on an `NSStatusItem`, not a window drawn to look like
//! one, so macOS does what a menu-bar menu should: the icon stays highlighted
//! while the menu is open (with no flicker), an auto-hidden menu bar stays shown,
//! keyboard navigation works, and the menu opens on the display that was clicked.
//!
//! The menu is rebuilt every time it opens (`menuNeedsUpdate:`), so the lanes and
//! their "open" marks are always current. Choosing a lane launches it; the "…"
//! items open the manager window, where lanes are added, renamed and removed.

use std::cell::RefCell;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{
    define_class, msg_send, sel, AnyThread, ClassType, DefinedClass, MainThreadMarker,
    MainThreadOnly,
};
use objc2_app_kit::{
    NSAlert, NSApplication, NSBezierPath, NSColor, NSControlStateValueOn, NSImage, NSMenu,
    NSMenuDelegate, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSPoint, NSRect, NSSize, NSString};
use tauri::AppHandle;

use crate::{apps, profiles};

pub struct Ivars {
    app: AppHandle,
    /// Profile ids of the lane items, indexed by each item's tag.
    lane_ids: RefCell<Vec<String>>,
    /// App keys of the "Add Current … Session…" items, indexed by tag.
    adopt_keys: RefCell<Vec<String>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and Controller has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "LanesMenuController"]
    #[ivars = Ivars]
    pub struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSMenuDelegate for Controller {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            self.rebuild(menu);
        }
    }

    impl Controller {
        #[unsafe(method(openLane:))]
        fn open_lane(&self, sender: &NSMenuItem) {
            let id = self.ivars().lane_ids.borrow().get(sender.tag() as usize).cloned();
            if let Some(id) = id {
                if let Err(e) = crate::launch_lane(&id) {
                    alert(self.mtm(), "Couldn't open this lane", &e);
                }
            }
        }

        #[unsafe(method(addCurrentSession:))]
        fn add_current_session(&self, sender: &NSMenuItem) {
            let key = self.ivars().adopt_keys.borrow().get(sender.tag() as usize).cloned();
            if let Some(key) = key {
                self.open_manager(format!("adopt:{key}"));
            }
        }

        #[unsafe(method(addLane:))]
        fn add_lane(&self, _sender: &NSMenuItem) {
            self.open_manager("add".into());
        }

        #[unsafe(method(manageLanes:))]
        fn manage_lanes(&self, _sender: &NSMenuItem) {
            self.open_manager("manage".into());
        }

        #[unsafe(method(copySetup:))]
        fn copy_setup(&self, _sender: &NSMenuItem) {
            self.open_manager("copy".into());
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &NSMenuItem) {
            defer(&self.ivars().app, |app| app.exit(0));
        }
    }
);

impl Controller {
    fn new(mtm: MainThreadMarker, app: AppHandle) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            app,
            lane_ids: RefCell::default(),
            adopt_keys: RefCell::default(),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn open_manager(&self, view: String) {
        defer(&self.ivars().app, move |app| crate::open_manager(app, &view));
    }

    fn rebuild(&self, menu: &NSMenu) {
        let mtm = self.mtm();
        menu.removeAllItems();

        let store = profiles::load();
        let running = crate::compute_running_dirs();
        let adoptable = crate::adoptable(&store.profiles, &running);

        menu.addItem(&section_header("Lanes", mtm));
        if store.profiles.is_empty() {
            let empty = self.item("No lanes yet", None, "");
            empty.setEnabled(false);
            menu.addItem(&empty);
        }
        let mut ids = Vec::new();
        for (i, p) in store.profiles.iter().enumerate() {
            let app = apps::find(&p.app);
            let open = running.contains(&p.data_dir);
            let mut details = vec![app.map_or(p.app.as_str(), |a| a.name).to_string()];
            if p.is_default {
                details.push("current session".into());
            }
            if open {
                details.push("open".into());
            }

            let item = self.item(&p.label, Some(sel!(openLane:)), "");
            item.setTag(i as isize);
            item.setImage(Some(&color_dot(&p.color)));
            describe(&item, &p.label, &details.join(" · "));
            if open {
                item.setState(NSControlStateValueOn);
            }
            item.setEnabled(app.is_some_and(|a| a.installed()));
            menu.addItem(&item);
            ids.push(p.id.clone());
        }
        *self.ivars().lane_ids.borrow_mut() = ids;

        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let mut keys = Vec::new();
        for (i, a) in adoptable.iter().enumerate() {
            let title = format!("Add Current {} Session…", a.name);
            let item = self.item(&title, Some(sel!(addCurrentSession:)), "");
            item.setTag(i as isize);
            menu.addItem(&item);
            keys.push(a.key.clone());
        }
        *self.ivars().adopt_keys.borrow_mut() = keys;
        menu.addItem(&self.item("Add Lane…", Some(sel!(addLane:)), ""));
        menu.addItem(&self.item("Manage Lanes…", Some(sel!(manageLanes:)), ""));
        if store.profiles.iter().filter(|p| p.app == "claude").count() >= 2 {
            menu.addItem(&self.item("Copy Setup Between Lanes…", Some(sel!(copySetup:)), ""));
        }

        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&self.item("Quit Lanes", Some(sel!(quit:)), "q"));
    }

    /// A menu item whose action, if any, is sent to this controller.
    fn item(&self, title: &str, action: Option<Sel>, key: &str) -> Retained<NSMenuItem> {
        // SAFETY: every action passed here is a method defined on Controller.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(self.mtm()),
                &NSString::from_str(title),
                action,
                &NSString::from_str(key),
            )
        };
        if action.is_some() {
            let target: &AnyObject = self;
            // SAFETY: the controller lives as long as the menu (see `install`).
            unsafe { item.setTarget(Some(target)) };
        }
        item
    }
}

/// A small, greyed section title ("Lanes"). Section headers are macOS 14+;
/// older systems get a disabled item instead.
fn section_header(title: &str, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    let title = NSString::from_str(title);
    if NSMenuItem::class()
        .metaclass()
        .responds_to(sel!(sectionHeaderWithTitle:))
    {
        return NSMenuItem::sectionHeaderWithTitle(&title, mtm);
    }
    let item = NSMenuItem::new(mtm);
    item.setTitle(&title);
    item.setEnabled(false);
    item
}

/// Show `details` ("Claude · open") under the title on macOS 14.4+, which has
/// menu-item subtitles, or after it on older systems.
fn describe(item: &NSMenuItem, title: &str, details: &str) {
    if item.respondsToSelector(sel!(setSubtitle:)) {
        item.setSubtitle(Some(&NSString::from_str(details)));
    } else {
        item.setTitle(&NSString::from_str(&format!("{title} — {details}")));
    }
}

/// The lane's colour as a dot, like a Finder tag in a menu.
fn color_dot(hex: &str) -> Retained<NSImage> {
    let (r, g, b) = parse_hex(hex).unwrap_or((0x59, 0x3e, 0xe8));
    let draw = RcBlock::new(move |rect: NSRect| -> Bool {
        let color = NSColor::colorWithSRGBRed_green_blue_alpha(
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
            1.0,
        );
        color.setFill();
        let inset = 2.5;
        let dot = NSRect::new(
            NSPoint::new(rect.origin.x + inset, rect.origin.y + inset),
            NSSize::new(rect.size.width - 2.0 * inset, rect.size.height - 2.0 * inset),
        );
        NSBezierPath::bezierPathWithOvalInRect(dot).fill();
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(16.0, 16.0), false, &draw)
}

fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?))
}

/// A plain alert, brought to the front (Lanes has no Dock icon to click).
fn alert(mtm: MainThreadMarker, title: &str, text: &str) {
    #[allow(deprecated)] // `activate()` is macOS 14+; this works on every version.
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(text));
    alert.runModal();
}

/// Run `f` as an ordinary event-loop task once this AppKit callback has
/// returned and the menu has closed, rather than re-entering Tauri from inside
/// the menu's action.
fn defer(app: &AppHandle, f: impl FnOnce(&AppHandle) + Send + 'static) {
    let app = app.clone();
    std::thread::spawn(move || {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || f(&handle));
    });
}

/// The status item, its menu and the menu's delegate (which `NSMenu` holds only
/// weakly), kept alive for as long as the app runs.
struct Installed {
    _item: Retained<NSStatusItem>,
    _menu: Retained<NSMenu>,
    _controller: Retained<Controller>,
}

thread_local! {
    static INSTALLED: RefCell<Option<Installed>> = const { RefCell::new(None) };
}

/// Put the Lanes item in the menu bar. Call once, on the main thread.
pub fn install(app: AppHandle) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let controller = Controller::new(mtm, app);

    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    menu.setDelegate(Some(ProtocolObject::from_ref(&*controller)));

    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    if let Some(button) = item.button(mtm) {
        // Monochrome template image: macOS tints it for light and dark menu bars.
        let data = NSData::with_bytes(include_bytes!("../icons/tray.png"));
        if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
            image.setSize(NSSize::new(18.0, 18.0));
            image.setTemplate(true);
            button.setImage(Some(&image));
        }
        button.setToolTip(Some(&NSString::from_str("Lanes")));
    }
    item.setMenu(Some(&menu));

    INSTALLED.with(|cell| {
        *cell.borrow_mut() = Some(Installed {
            _item: item,
            _menu: menu,
            _controller: controller,
        })
    });
}
