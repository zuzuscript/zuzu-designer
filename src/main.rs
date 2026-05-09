use gtk::gdk;
use gtk::glib::translate::FromGlibPtrNone;
use gtk::glib::{self, Type};
use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, CellRendererText, Dialog, DropDown, Entry,
    FileChooserAction, FileChooserNative, Frame, Grid, Label, Orientation, Paned, PolicyType,
    PopoverMenuBar, ResponseType, ScrolledWindow, TextBuffer, TextView, TreeIter, TreeStore,
    TreeView, TreeViewColumn, Widget,
};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use zuzu_rust::Runtime;

const APP_ID: &str = "org.zuzuscript.ZuzuDesigner";
const GUI_XML_NS: &str = "https://zuzulang.org/ns/std/gui";

type NodeRef = Rc<RefCell<ElementNode>>;

#[derive(Clone)]
struct ElementSpec {
    name: &'static str,
    attrs: &'static [&'static str],
}

#[derive(Clone)]
struct ElementNode {
    id: u32,
    tag: String,
    props: BTreeMap<String, String>,
    children: Vec<NodeRef>,
}

#[derive(Clone)]
struct Designer {
    root: NodeRef,
    next_id: Rc<Cell<u32>>,
    nodes: Rc<RefCell<HashMap<u32, NodeRef>>>,
    tree_store: TreeStore,
    tree_view: TreeView,
    preview_box: GtkBox,
    xml_buffer: TextBuffer,
    window: ApplicationWindow,
    runtime: Runtime,
    current_path: Rc<RefCell<Option<PathBuf>>>,
    syncing_tree: Rc<Cell<bool>>,
    tree_sync_pending: Rc<Cell<bool>>,
}

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &Application) {
    let root = new_node(1, "Window");
    {
        let mut root_mut = root.borrow_mut();
        root_mut
            .props
            .insert("title".to_owned(), "Untitled Window".to_owned());
        root_mut.props.insert("width".to_owned(), "800".to_owned());
        root_mut.props.insert("height".to_owned(), "600".to_owned());
        let vbox = new_node(2, "VBox");
        vbox.borrow_mut()
            .props
            .insert("padding".to_owned(), "12".to_owned());
        root_mut.children.push(vbox.clone());
        let label = new_node(3, "Label");
        label
            .borrow_mut()
            .props
            .insert("text".to_owned(), "Hello Zuzu".to_owned());
        vbox.borrow_mut().children.push(label);
        let button = new_node(4, "Button");
        button
            .borrow_mut()
            .props
            .insert("text".to_owned(), "OK".to_owned());
        vbox.borrow_mut().children.push(button);
    }

    let tree_store = TreeStore::new(&[Type::U32, Type::STRING]);
    let tree_view = TreeView::with_model(&tree_store);
    tree_view.set_headers_visible(false);
    tree_view.set_vexpand(true);
    tree_view.set_reorderable(true);

    let column = TreeViewColumn::new();
    let renderer = CellRendererText::new();
    column.pack_start(&renderer, true);
    column.add_attribute(&renderer, "text", 1);
    tree_view.append_column(&column);

    let preview_box = GtkBox::new(Orientation::Vertical, 8);
    preview_box.set_hexpand(true);
    preview_box.set_vexpand(true);

    let xml_buffer = TextBuffer::new(None);
    let xml_view = TextView::with_buffer(&xml_buffer);
    xml_view.set_editable(false);
    xml_view.set_monospace(true);
    xml_view.set_wrap_mode(gtk::WrapMode::None);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Zuzu Designer")
        .default_width(1180)
        .default_height(760)
        .build();

    let designer = Designer {
        root,
        next_id: Rc::new(Cell::new(5)),
        nodes: Rc::new(RefCell::new(HashMap::new())),
        tree_store,
        tree_view,
        preview_box,
        xml_buffer,
        window: window.clone(),
        runtime: Runtime::from_repo_root(&repo_root()),
        current_path: Rc::new(RefCell::new(None)),
        syncing_tree: Rc::new(Cell::new(false)),
        tree_sync_pending: Rc::new(Cell::new(false)),
    };

    let shell = GtkBox::new(Orientation::Vertical, 0);
    let menu_bar = build_menu_bar(&designer);
    shell.append(&menu_bar);

    let paned = Paned::new(Orientation::Horizontal);
    paned.set_wide_handle(true);
    paned.set_start_child(Some(&build_tree_pane(&designer)));
    paned.set_end_child(Some(&build_preview_pane(&designer, &xml_view)));
    paned.set_position(360);
    shell.append(&paned);

    window.set_child(Some(&shell));
    connect_tree_events(&designer);
    if let Some(path) = std::env::args().nth(1).filter(|arg| !arg.starts_with('-')) {
        designer.open_path(PathBuf::from(path));
    } else {
        designer.refresh();
    }
    window.present();
}

fn build_menu_bar(designer: &Designer) -> PopoverMenuBar {
    add_designer_action(designer, "open", |designer| designer.open_dialog());
    add_designer_action(designer, "save", |designer| designer.save());
    add_designer_action(designer, "save-as", |designer| designer.save_as_dialog());
    add_designer_action(designer, "quit", |designer| designer.window.close());
    add_designer_action(designer, "copy-window-xml", |designer| {
        designer.copy_window_xml()
    });
    add_designer_action(designer, "copy-widget-xml", |designer| {
        designer.copy_widget_xml()
    });
    add_designer_action(designer, "paste-window-xml", |designer| {
        designer.paste_window_xml()
    });
    add_designer_action(designer, "paste-widget-child-xml", |designer| {
        designer.paste_widget_xml_as_child()
    });
    add_designer_action(designer, "edit", |designer| designer.edit_selected());
    add_designer_action(designer, "move-up", |designer| designer.move_selected(-1));
    add_designer_action(designer, "move-down", |designer| designer.move_selected(1));
    add_designer_action(designer, "add-sibling", |designer| {
        designer.add_sibling_dialog()
    });
    add_designer_action(designer, "add-child", |designer| {
        designer.add_child_dialog()
    });
    add_designer_action(designer, "remove", |designer| designer.remove_selected());

    let root = gio::Menu::new();
    root.append_submenu(Some("File"), &file_menu());
    root.append_submenu(Some("Edit"), &edit_menu());
    root.append_submenu(Some("Widget"), &widget_menu());
    PopoverMenuBar::from_model(Some(&root))
}

fn add_designer_action<F>(designer: &Designer, name: &str, handler: F)
where
    F: Fn(&Designer) + 'static,
{
    let action = gio::SimpleAction::new(name, None);
    let action_designer = designer.clone();
    action.connect_activate(move |_, _| handler(&action_designer));
    designer.window.add_action(&action);
}

fn file_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("Open"), Some("win.open"));
    menu.append(Some("Save"), Some("win.save"));
    menu.append(Some("Save As"), Some("win.save-as"));

    let quit = gio::Menu::new();
    quit.append(Some("Quit"), Some("win.quit"));
    menu.append_section(None, &quit);
    menu
}

fn edit_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("Copy Window XML"), Some("win.copy-window-xml"));
    menu.append(Some("Copy Widget XML"), Some("win.copy-widget-xml"));
    menu.append(Some("Paste Window XML"), Some("win.paste-window-xml"));
    menu.append(
        Some("Paste Widget XML as Child"),
        Some("win.paste-widget-child-xml"),
    );
    menu
}

fn widget_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("Edit"), Some("win.edit"));
    menu.append(Some("Move Up"), Some("win.move-up"));
    menu.append(Some("Move Down"), Some("win.move-down"));

    let add = gio::Menu::new();
    add.append(Some("Add Sibling"), Some("win.add-sibling"));
    add.append(Some("Add Child"), Some("win.add-child"));
    menu.append_section(None, &add);

    let remove = gio::Menu::new();
    remove.append(Some("Remove"), Some("win.remove"));
    menu.append_section(None, &remove);
    menu
}

fn build_tree_pane(designer: &Designer) -> ScrolledWindow {
    ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .min_content_width(320)
        .child(&designer.tree_view)
        .build()
}

fn build_preview_pane(designer: &Designer, xml_view: &TextView) -> GtkBox {
    let pane = GtkBox::new(Orientation::Vertical, 8);
    pane.set_margin_top(8);
    pane.set_margin_bottom(8);
    pane.set_margin_start(8);
    pane.set_margin_end(8);

    let preview_frame = Frame::builder().label("Preview").build();
    let preview_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(&designer.preview_box)
        .build();
    preview_frame.set_child(Some(&preview_scroll));

    let xml_frame = Frame::builder().label("Generated XML").build();
    let xml_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .min_content_height(180)
        .child(xml_view)
        .build();
    xml_frame.set_child(Some(&xml_scroll));

    pane.append(&preview_frame);
    pane.append(&xml_frame);
    pane
}

fn connect_tree_events(designer: &Designer) {
    {
        let designer = designer.clone();
        let tree_view = designer.tree_view.clone();
        tree_view.connect_row_activated(move |_, _, _| designer.edit_selected());
    }

    {
        let designer = designer.clone();
        let tree_store = designer.tree_store.clone();
        tree_store.connect_row_changed(move |_, _, _| designer.sync_after_tree_drag());
    }
    {
        let designer = designer.clone();
        let tree_store = designer.tree_store.clone();
        tree_store.connect_row_deleted(move |_, _| designer.sync_after_tree_drag());
    }
    {
        let designer = designer.clone();
        let tree_store = designer.tree_store.clone();
        tree_store.connect_row_inserted(move |_, _, _| designer.sync_after_tree_drag());
    }
}

impl Designer {
    fn refresh(&self) {
        self.syncing_tree.set(true);
        self.nodes.borrow_mut().clear();
        self.tree_store.clear();
        self.append_tree_node(&self.root, None);
        self.syncing_tree.set(false);
        self.tree_view.expand_all();
        self.refresh_preview();
        self.xml_buffer.set_text(&to_xml(&self.root));
        self.update_window_title();
    }

    fn append_tree_node(&self, node: &NodeRef, parent: Option<&TreeIter>) {
        let borrowed = node.borrow();
        self.nodes.borrow_mut().insert(borrowed.id, node.clone());
        let iter = self.tree_store.append(parent);
        let label = node_label(&borrowed);
        self.tree_store
            .set(&iter, &[(0, &borrowed.id), (1, &label)]);
        for child in &borrowed.children {
            self.append_tree_node(child, Some(&iter));
        }
    }

    fn selected_node(&self) -> Option<NodeRef> {
        let (model, iter) = self.tree_view.selection().selected()?;
        let id = model.get::<u32>(&iter, 0);
        self.nodes.borrow().get(&id).cloned()
    }

    fn selected_id(&self) -> Option<u32> {
        self.selected_node().map(|node| node.borrow().id)
    }

    fn add_child_dialog(&self) {
        let parent = self.selected_node().unwrap_or_else(|| self.root.clone());
        self.choose_element("Add Child", move |designer, tag| {
            let child = new_node(designer.allocate_id(), &tag);
            apply_initial_props(&child);
            parent.borrow_mut().children.push(child);
            designer.refresh();
        });
    }

    fn add_sibling_dialog(&self) {
        let Some(selected_id) = self.selected_id() else {
            return;
        };
        if selected_id == self.root.borrow().id {
            return;
        }
        self.choose_element("Add Sibling", move |designer, tag| {
            let sibling = new_node(designer.allocate_id(), &tag);
            apply_initial_props(&sibling);
            if let Some((parent, index)) = find_parent(&designer.root, selected_id) {
                parent.borrow_mut().children.insert(index + 1, sibling);
                designer.refresh();
            }
        });
    }

    fn choose_element<F>(&self, title: &str, handler: F)
    where
        F: Fn(Designer, String) + 'static,
    {
        let dialog = Dialog::builder()
            .title(title)
            .transient_for(&self.window)
            .modal(true)
            .default_width(320)
            .build();
        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Add", ResponseType::Accept);

        let area = dialog.content_area();
        area.set_margin_top(12);
        area.set_margin_bottom(12);
        area.set_margin_start(12);
        area.set_margin_end(12);
        let names: Vec<&str> = element_specs().iter().map(|spec| spec.name).collect();
        let dropdown = DropDown::from_strings(&names);
        dropdown.set_selected(1);
        area.append(&dropdown);

        let designer = self.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                let selected = dropdown.selected() as usize;
                if let Some(tag) = names.get(selected) {
                    handler(designer.clone(), (*tag).to_owned());
                }
            }
            dialog.close();
        });
        dialog.present();
    }

    fn edit_selected(&self) {
        let Some(node) = self.selected_node() else {
            return;
        };
        let borrowed = node.borrow();
        let title = format!("Edit {}", borrowed.tag);
        let attrs = attrs_for(&borrowed.tag);
        let props = borrowed.props.clone();
        drop(borrowed);

        let dialog = Dialog::builder()
            .title(&title)
            .transient_for(&self.window)
            .modal(true)
            .default_width(520)
            .build();
        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Apply", ResponseType::Accept);

        let area = dialog.content_area();
        area.set_margin_top(12);
        area.set_margin_bottom(12);
        area.set_margin_start(12);
        area.set_margin_end(12);

        let grid = Grid::builder()
            .column_spacing(10)
            .row_spacing(6)
            .hexpand(true)
            .build();
        let mut entries = Vec::new();
        for (row, attr) in attrs.iter().enumerate() {
            let label = Label::new(Some(attr));
            label.set_halign(gtk::Align::Start);
            let entry = Entry::new();
            entry.set_hexpand(true);
            entry.set_text(props.get(*attr).map(String::as_str).unwrap_or(""));
            grid.attach(&label, 0, row as i32, 1, 1);
            grid.attach(&entry, 1, row as i32, 1, 1);
            entries.push(((*attr).to_owned(), entry));
        }
        area.append(&grid);

        let designer = self.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                let mut node = node.borrow_mut();
                node.props.clear();
                for (name, entry) in &entries {
                    let value = entry.text().trim().to_owned();
                    if !value.is_empty() {
                        node.props.insert(name.clone(), value);
                    }
                }
                drop(node);
                designer.refresh();
            }
            dialog.close();
        });
        dialog.present();
    }

    fn remove_selected(&self) {
        let Some(selected_id) = self.selected_id() else {
            return;
        };
        if selected_id == self.root.borrow().id {
            return;
        }
        if let Some((parent, index)) = find_parent(&self.root, selected_id) {
            parent.borrow_mut().children.remove(index);
            self.refresh();
        }
    }

    fn move_selected(&self, offset: isize) {
        let Some(selected_id) = self.selected_id() else {
            return;
        };
        if let Some((parent, index)) = find_parent(&self.root, selected_id) {
            let mut parent = parent.borrow_mut();
            let new_index = index as isize + offset;
            if new_index < 0 || new_index >= parent.children.len() as isize {
                return;
            }
            parent.children.swap(index, new_index as usize);
            drop(parent);
            self.refresh();
        }
    }

    fn copy_window_xml(&self) {
        self.copy_to_clipboard(to_xml(&self.root));
    }

    fn copy_widget_xml(&self) {
        if let Some(node) = self.selected_node() {
            self.copy_to_clipboard(to_xml(&node));
        }
    }

    fn paste_window_xml(&self) {
        self.read_clipboard_text(
            "Paste Window XML Failed",
            |designer, text| match parse_gui_xml(text) {
                Ok(root) => designer.replace_document(root, None),
                Err(err) => designer.show_error("Paste Window XML Failed", &err),
            },
        );
    }

    fn paste_widget_xml_as_child(&self) {
        let parent = self.selected_node().unwrap_or_else(|| self.root.clone());
        self.read_clipboard_text("Paste Widget XML Failed", move |designer, text| {
            match parse_widget_xml(text) {
                Ok(node) => {
                    if node.borrow().tag == "Window" {
                        designer.show_error(
                            "Paste Widget XML Failed",
                            "Window XML cannot be pasted as a child widget.",
                        );
                        return;
                    }
                    designer.assign_new_ids(&node);
                    parent.borrow_mut().children.push(node);
                    designer.refresh();
                }
                Err(err) => designer.show_error("Paste Widget XML Failed", &err),
            }
        });
    }

    fn copy_to_clipboard(&self, xml: String) {
        let Some(display) = gdk::Display::default() else {
            self.show_error("Copy Failed", "No display clipboard is available.");
            return;
        };
        display.clipboard().set_text(&xml);
    }

    fn read_clipboard_text<F>(&self, title: &'static str, handler: F)
    where
        F: FnOnce(&Designer, &str) + 'static,
    {
        let Some(display) = gdk::Display::default() else {
            self.show_error(title, "No display clipboard is available.");
            return;
        };
        let designer = self.clone();
        display.clipboard().read_text_async(
            None::<&gio::Cancellable>,
            move |result| match result {
                Ok(Some(text)) => handler(&designer, text.as_str()),
                Ok(None) => designer.show_error(title, "The clipboard does not contain text."),
                Err(err) => designer.show_error(title, &err.to_string()),
            },
        );
    }

    fn open_dialog(&self) {
        let dialog = FileChooserNative::new(
            Some("Open Zuzu GUI XML"),
            Some(&self.window),
            FileChooserAction::Open,
            Some("Open"),
            Some("Cancel"),
        );
        let designer = self.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                if let Some(path) = dialog.file().and_then(|file| file.path()) {
                    designer.open_path(path);
                }
            }
            dialog.destroy();
        });
        dialog.show();
    }

    fn open_path(&self, path: PathBuf) {
        match fs::read_to_string(&path)
            .map_err(|err| err.to_string())
            .and_then(|xml| parse_gui_xml(&xml))
        {
            Ok(root) => {
                self.replace_document(root, Some(path));
            }
            Err(err) => {
                self.show_error("Open Failed", &err);
                self.refresh();
            }
        }
    }

    fn save(&self) {
        if let Some(path) = self.current_path.borrow().clone() {
            self.save_to_path(path);
        } else {
            self.save_as_dialog();
        }
    }

    fn save_as_dialog(&self) {
        let dialog = FileChooserNative::new(
            Some("Save Zuzu GUI XML"),
            Some(&self.window),
            FileChooserAction::Save,
            Some("Save"),
            Some("Cancel"),
        );
        if let Some(path) = self.current_path.borrow().as_ref() {
            let _ = dialog.set_file(&gio::File::for_path(path));
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                dialog.set_current_name(name);
            }
        } else {
            dialog.set_current_name("form.xml");
        }
        let designer = self.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                if let Some(path) = dialog.file().and_then(|file| file.path()) {
                    designer.save_to_path(path);
                }
            }
            dialog.destroy();
        });
        dialog.show();
    }

    fn save_to_path(&self, path: PathBuf) {
        match fs::write(&path, to_xml(&self.root)) {
            Ok(()) => {
                *self.current_path.borrow_mut() = Some(path);
                self.update_window_title();
            }
            Err(err) => {
                self.show_error("Save Failed", &err.to_string());
            }
        }
    }

    fn replace_document(&self, root: NodeRef, path: Option<PathBuf>) {
        *self.root.borrow_mut() = root.borrow().clone();
        *self.current_path.borrow_mut() = path;
        self.reindex_document();
        self.refresh();
    }

    fn reindex_document(&self) {
        self.nodes.borrow_mut().clear();
        let mut max_id = 0;
        reindex_node(&self.root, &self.nodes, &mut max_id);
        self.next_id.set(max_id + 1);
    }

    fn assign_new_ids(&self, node: &NodeRef) {
        node.borrow_mut().id = self.allocate_id();
        let children = node.borrow().children.clone();
        for child in children {
            self.assign_new_ids(&child);
        }
    }

    fn sync_after_tree_drag(&self) {
        if self.syncing_tree.get() {
            return;
        }
        if self.tree_sync_pending.replace(true) {
            return;
        };
        let designer = self.clone();
        glib::idle_add_local_once(move || {
            designer.tree_sync_pending.set(false);
            designer.sync_tree_now();
        });
    }

    fn sync_tree_now(&self) {
        if self.syncing_tree.get() {
            return;
        }
        if self.tree_store.iter_n_children(None) != 1 {
            self.refresh();
            return;
        }
        let Some(root) = self.tree_root_from_store() else {
            self.refresh();
            return;
        };
        if root.borrow().tag != "Window" {
            self.refresh();
            return;
        }
        *self.root.borrow_mut() = root.borrow().clone();
        self.reindex_document();
        self.refresh();
    }

    fn tree_root_from_store(&self) -> Option<NodeRef> {
        let root_iter = self.tree_store.iter_nth_child(None, 0)?;
        self.node_from_tree_iter(&root_iter)
    }

    fn node_from_tree_iter(&self, iter: &TreeIter) -> Option<NodeRef> {
        let id = self.tree_store.get::<u32>(iter, 0);
        let node = self.nodes.borrow().get(&id).cloned()?;
        let new_node = Rc::new(RefCell::new(ElementNode {
            id,
            tag: node.borrow().tag.clone(),
            props: node.borrow().props.clone(),
            children: Vec::new(),
        }));
        let mut children = Vec::new();
        if let Some(mut child_iter) = self.tree_store.iter_children(Some(iter)) {
            loop {
                if let Some(child) = self.node_from_tree_iter(&child_iter) {
                    children.push(child);
                }
                if !self.tree_store.iter_next(&mut child_iter) {
                    break;
                }
            }
        }
        new_node.borrow_mut().children = children;
        Some(new_node)
    }

    fn show_error(&self, title: &str, message: &str) {
        let dialog = Dialog::builder()
            .title(title)
            .transient_for(&self.window)
            .modal(true)
            .default_width(420)
            .build();
        dialog.add_button("OK", ResponseType::Accept);
        let area = dialog.content_area();
        area.set_margin_top(12);
        area.set_margin_bottom(12);
        area.set_margin_start(12);
        area.set_margin_end(12);
        area.append(&Label::new(Some(message)));
        dialog.connect_response(|dialog, _| dialog.close());
        dialog.present();
    }

    fn update_window_title(&self) {
        let title = self
            .current_path
            .borrow()
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(|name| format!("Zuzu Designer - {name}"))
            .unwrap_or_else(|| "Zuzu Designer".to_owned());
        self.window.set_title(Some(&title));
    }

    fn refresh_preview(&self) {
        while let Some(child) = self.preview_box.first_child() {
            self.preview_box.remove(&child);
        }
        match self.runtime.gui_xml_preview_widget(&to_xml(&self.root)) {
            Ok(native) if !native.is_null() => {
                let widget: Widget =
                    unsafe { Widget::from_glib_none(native.cast::<gtk::ffi::GtkWidget>()) };
                self.preview_box.append(&widget);
            }
            Ok(_) => {
                self.preview_box
                    .append(&preview_placeholder("Preview unavailable"));
            }
            Err(err) => {
                self.preview_box
                    .append(&preview_placeholder(&format!("Preview error: {err}")));
            }
        }
    }

    fn allocate_id(&self) -> u32 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        id
    }
}

fn repo_root() -> PathBuf {
    let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    find_repo_root(&start).unwrap_or(start)
}

fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut current = start;
    loop {
        if current.join("modules/std/gui.zzm").is_file()
            && current.join("extras/zuzu-rust/Cargo.toml").is_file()
        {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn new_node(id: u32, tag: &str) -> NodeRef {
    Rc::new(RefCell::new(ElementNode {
        id,
        tag: tag.to_owned(),
        props: BTreeMap::new(),
        children: Vec::new(),
    }))
}

fn apply_initial_props(node: &NodeRef) {
    let mut node = node.borrow_mut();
    match node.tag.as_str() {
        "Window" => {
            node.props
                .insert("title".to_owned(), "Untitled Window".to_owned());
            node.props.insert("width".to_owned(), "800".to_owned());
            node.props.insert("height".to_owned(), "600".to_owned());
        }
        "VBox" | "HBox" => {
            node.props.insert("gap".to_owned(), "6".to_owned());
            node.props.insert("padding".to_owned(), "8".to_owned());
        }
        "Frame" => {
            node.props.insert("label".to_owned(), "Frame".to_owned());
        }
        "Label" => {
            node.props.insert("text".to_owned(), "Label".to_owned());
        }
        "Text" | "RichText" => {
            node.props.insert("value".to_owned(), "Text".to_owned());
        }
        "Image" => {
            node.props.insert("alt".to_owned(), "Image".to_owned());
        }
        "Input" => {
            node.props
                .insert("placeholder".to_owned(), "Input".to_owned());
        }
        "DatePicker" => {
            node.props
                .insert("value".to_owned(), "2026-04-27".to_owned());
        }
        "Checkbox" | "Radio" => {
            let label = node.tag.clone();
            node.props.insert("label".to_owned(), label);
        }
        "RadioGroup" => {
            node.props.insert("name".to_owned(), "choice".to_owned());
        }
        "Menu" => {
            node.props.insert("text".to_owned(), "Menu".to_owned());
        }
        "MenuItem" => {
            node.props.insert("text".to_owned(), "Item".to_owned());
        }
        "Button" => {
            node.props.insert("text".to_owned(), "Button".to_owned());
        }
        "Slider" | "Progress" => {
            node.props.insert("value".to_owned(), "50".to_owned());
        }
        "Tabs" => {
            node.props.insert("selected".to_owned(), "tab1".to_owned());
        }
        "Tab" => {
            node.props.insert("title".to_owned(), "Tab".to_owned());
            node.props.insert("value".to_owned(), "tab1".to_owned());
        }
        _ => {}
    }
}

fn element_specs() -> Vec<ElementSpec> {
    vec![
        ElementSpec {
            name: "Window",
            attrs: &[
                "id",
                "title",
                "width",
                "height",
                "resizable",
                "modal",
                "visible",
                "enabled",
                "disabled",
            ],
        },
        ElementSpec {
            name: "VBox",
            attrs: &[
                "id", "align", "gap", "padding", "visible", "enabled", "disabled", "width",
                "height",
            ],
        },
        ElementSpec {
            name: "HBox",
            attrs: &[
                "id", "align", "gap", "padding", "visible", "enabled", "disabled", "width",
                "height",
            ],
        },
        ElementSpec {
            name: "Frame",
            attrs: &[
                "id",
                "label",
                "collapsible",
                "collapsed",
                "visible",
                "enabled",
                "disabled",
                "width",
                "height",
            ],
        },
        ElementSpec {
            name: "Menu",
            attrs: &["id", "text", "visible", "enabled", "disabled"],
        },
        ElementSpec {
            name: "MenuItem",
            attrs: &["id", "text", "visible", "enabled", "disabled"],
        },
        ElementSpec {
            name: "Label",
            attrs: &[
                "id", "text", "for", "visible", "enabled", "disabled", "width", "height",
            ],
        },
        ElementSpec {
            name: "Text",
            attrs: &[
                "id",
                "value",
                "multiline",
                "readonly",
                "wrap",
                "visible",
                "enabled",
                "disabled",
                "width",
                "height",
            ],
        },
        ElementSpec {
            name: "RichText",
            attrs: &[
                "id",
                "value",
                "format",
                "multiline",
                "readonly",
                "visible",
                "enabled",
                "disabled",
                "width",
                "height",
            ],
        },
        ElementSpec {
            name: "Image",
            attrs: &[
                "id", "src", "alt", "fit", "visible", "enabled", "disabled", "width", "height",
            ],
        },
        ElementSpec {
            name: "Input",
            attrs: &[
                "id",
                "value",
                "placeholder",
                "multiline",
                "readonly",
                "password",
                "required",
                "visible",
                "enabled",
                "disabled",
                "width",
                "height",
            ],
        },
        ElementSpec {
            name: "DatePicker",
            attrs: &[
                "id",
                "value",
                "min",
                "max",
                "first_day_of_week",
                "visible",
                "enabled",
                "disabled",
                "width",
                "height",
            ],
        },
        ElementSpec {
            name: "Checkbox",
            attrs: &[
                "id",
                "label",
                "checked",
                "indeterminate",
                "visible",
                "enabled",
                "disabled",
                "width",
                "height",
            ],
        },
        ElementSpec {
            name: "Radio",
            attrs: &[
                "id", "label", "value", "group", "checked", "visible", "enabled", "disabled",
                "width", "height",
            ],
        },
        ElementSpec {
            name: "RadioGroup",
            attrs: &[
                "id", "name", "value", "visible", "enabled", "disabled", "width", "height",
            ],
        },
        ElementSpec {
            name: "Select",
            attrs: &[
                "id", "value", "multiple", "visible", "enabled", "disabled", "width", "height",
            ],
        },
        ElementSpec {
            name: "Button",
            attrs: &[
                "id", "text", "variant", "visible", "enabled", "disabled", "width", "height",
            ],
        },
        ElementSpec {
            name: "Separator",
            attrs: &[
                "id",
                "orientation",
                "visible",
                "enabled",
                "disabled",
                "width",
                "height",
            ],
        },
        ElementSpec {
            name: "Slider",
            attrs: &[
                "id",
                "value",
                "min",
                "max",
                "step",
                "orientation",
                "readonly",
                "visible",
                "enabled",
                "disabled",
                "width",
                "height",
            ],
        },
        ElementSpec {
            name: "Progress",
            attrs: &[
                "id",
                "value",
                "min",
                "max",
                "indeterminate",
                "show_text",
                "visible",
                "enabled",
                "disabled",
                "width",
                "height",
            ],
        },
        ElementSpec {
            name: "Tabs",
            attrs: &[
                "id",
                "selected",
                "placement",
                "visible",
                "enabled",
                "disabled",
                "width",
                "height",
            ],
        },
        ElementSpec {
            name: "Tab",
            attrs: &[
                "id", "title", "value", "selected", "closable", "icon", "visible", "enabled",
                "disabled", "width", "height",
            ],
        },
        ElementSpec {
            name: "ListView",
            attrs: &[
                "id",
                "selected_index",
                "multiple",
                "visible",
                "enabled",
                "disabled",
                "width",
                "height",
            ],
        },
        ElementSpec {
            name: "TreeView",
            attrs: &[
                "id",
                "selected_path",
                "multiple",
                "visible",
                "enabled",
                "disabled",
                "width",
                "height",
            ],
        },
    ]
}

fn attrs_for(tag: &str) -> &'static [&'static str] {
    element_specs()
        .into_iter()
        .find(|spec| spec.name == tag)
        .map(|spec| spec.attrs)
        .unwrap_or(&["id", "visible", "enabled", "disabled"])
}

fn node_label(node: &ElementNode) -> String {
    let summary = ["id", "title", "text", "label", "value", "src"]
        .iter()
        .find_map(|key| node.props.get(*key).map(|value| format!("{key}={value}")));
    match summary {
        Some(summary) => format!("{} ({})", node.tag, summary),
        None => node.tag.clone(),
    }
}

fn find_parent(root: &NodeRef, child_id: u32) -> Option<(NodeRef, usize)> {
    let root_borrowed = root.borrow();
    for (index, child) in root_borrowed.children.iter().enumerate() {
        if child.borrow().id == child_id {
            return Some((root.clone(), index));
        }
        if let Some(found) = find_parent(child, child_id) {
            return Some(found);
        }
    }
    None
}

fn preview_placeholder(label: &str) -> Label {
    let placeholder = Label::new(Some(label));
    placeholder.set_margin_top(8);
    placeholder.set_margin_bottom(8);
    placeholder.set_margin_start(8);
    placeholder.set_margin_end(8);
    placeholder.add_css_class("dim-label");
    placeholder
}

fn parse_gui_xml(xml: &str) -> Result<NodeRef, String> {
    let node = parse_widget_xml(xml)?;
    if node.borrow().tag != "Window" {
        return Err("GUI XML root element must be Window".to_owned());
    }
    Ok(node)
}

fn parse_widget_xml(xml: &str) -> Result<NodeRef, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|err| err.to_string())?;
    let root = doc.root_element();
    let mut next_id = 1;
    parse_xml_node(root, &mut next_id)
}

fn parse_xml_node(node: roxmltree::Node<'_, '_>, next_id: &mut u32) -> Result<NodeRef, String> {
    let ns = node.tag_name().namespace().unwrap_or("");
    if !ns.is_empty() && ns != GUI_XML_NS {
        return Err(format!("unsupported GUI XML namespace '{ns}'"));
    }

    let tag = node.tag_name().name();
    if !is_supported_element(tag) {
        return Err(format!("unsupported GUI XML element '{tag}'"));
    }

    let parsed = new_node(*next_id, tag);
    *next_id += 1;
    let allowed = attrs_for(tag);
    for attr in node.attributes() {
        if attr.namespace().is_some() {
            return Err(format!(
                "{} does not accept namespaced XML attribute '{}'",
                tag,
                attr.name()
            ));
        }
        let name = attr.name();
        if name == "xmlns" || name.starts_with("xmlns:") {
            continue;
        }
        if !name.starts_with("meta.") && !allowed.contains(&name) {
            return Err(format!("{} does not accept XML attribute '{}'", tag, name));
        }
        parsed
            .borrow_mut()
            .props
            .insert(name.to_owned(), attr.value().to_owned());
    }

    let mut children = Vec::new();
    for child in node.children().filter(|child| child.is_element()) {
        children.push(parse_xml_node(child, next_id)?);
    }
    parsed.borrow_mut().children = children;
    Ok(parsed)
}

fn is_supported_element(tag: &str) -> bool {
    element_specs().iter().any(|spec| spec.name == tag)
}

fn reindex_node(node: &NodeRef, nodes: &Rc<RefCell<HashMap<u32, NodeRef>>>, max_id: &mut u32) {
    let id = node.borrow().id;
    *max_id = (*max_id).max(id);
    nodes.borrow_mut().insert(id, node.clone());
    for child in &node.borrow().children {
        reindex_node(child, nodes, max_id);
    }
}

fn to_xml(root: &NodeRef) -> String {
    let mut out = String::new();
    write_node_xml(root, 0, &mut out);
    out
}

fn write_node_xml(node: &NodeRef, depth: usize, out: &mut String) {
    let node = node.borrow();
    let indent = "\t".repeat(depth);
    out.push_str(&indent);
    out.push('<');
    out.push_str(&node.tag);

    if depth == 0 {
        out.push_str(" xmlns=\"");
        out.push_str(GUI_XML_NS);
        out.push('"');
    }

    for (name, value) in &node.props {
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        out.push_str(&escape_xml(value));
        out.push('"');
    }

    if node.children.is_empty() {
        out.push_str(" />\n");
        return;
    }

    out.push_str(">\n");
    for child in &node.children {
        write_node_xml(child, depth + 1, out);
    }
    out.push_str(&indent);
    out.push_str("</");
    out.push_str(&node.tag);
    out.push_str(">\n");
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
