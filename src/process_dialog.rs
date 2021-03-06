use glib::object::Cast;
use gtk::{
    self, AdjustmentExt, BoxExt, ButtonExt, ContainerExt, DialogExt, LabelExt, ScrolledWindowExt
};
use gtk::{WidgetExt, GtkWindowExt};
use pango;
use sysinfo::{self, Pid, ProcessExt};

use std::cell::RefCell;
use std::iter;
use std::rc::Rc;

use graph::{Connecter, Graph};
use notebook::NoteBook;
use utils::{connect_graph, format_number, RotateVec};

#[allow(dead_code)]
pub struct ProcDialog {
    working_directory: gtk::Label,
    memory_usage: gtk::Label,
    cpu_usage: gtk::Label,
    run_time: gtk::Label,
    pub popup: gtk::Window,
    pub pid: Pid,
    notebook: NoteBook,
    ram_usage_history: Rc<RefCell<Graph>>,
    cpu_usage_history: Rc<RefCell<Graph>>,
}

impl ProcDialog {
    pub fn update(&self, process: &sysinfo::Process, running_since: u64, start_time: u64) {
        self.working_directory.set_text(&process.cwd().display().to_string());
        self.memory_usage.set_text(&format_number(process.memory() << 10)); // * 1_024
        self.cpu_usage.set_text(&format!("{:.1}%", process.cpu_usage()));
        let running_since = compute_running_since(process, start_time, running_since);
        self.run_time.set_text(&format_time(running_since));

        let mut t = self.ram_usage_history.borrow_mut();
        t.data[0].move_start();
        *t.data[0].get_mut(0).expect("cannot get data 0") = process.memory() as f64;
        t.invalidate();
        let mut t = self.cpu_usage_history.borrow_mut();
        t.data[0].move_start();
        *t.data[0].get_mut(0).expect("cannot get data 0") = process.cpu_usage() as f64;
        t.invalidate();
    }
}

fn format_time(t: u64) -> String {
    format!("{}{}{}{}s",
            {
                let days = t / 86_400;
                if days > 0 {
                    format!("{}d ", days)
                } else {
                    "".to_owned()
                }
            },
            {
                let hours = t / 3_600 % 24;
                if hours > 0 {
                    format!("{}h ", hours)
                } else {
                    "".to_owned()
                }
            },
            {
                let minutes = t / 60 % 60;
                if minutes > 0 {
                    format!("{}m ", minutes)
                } else {
                    "".to_owned()
                }
            },
            t % 60)
}

fn create_and_add_new_label(scroll: &gtk::Box, title: &str, text: &str) -> gtk::Label {
    let horizontal_layout = gtk::Box::new(gtk::Orientation::Horizontal, 0);

    horizontal_layout.set_margin_top(5);
    horizontal_layout.set_margin_bottom(5);
    horizontal_layout.set_margin_end(5);
    horizontal_layout.set_margin_start(5);

    let label = gtk::Label::new(None);
    label.set_justify(gtk::Justification::Left);
    label.set_markup(&format!("<b>{}:</b> ", title));

    let text = gtk::Label::new(text);
    text.set_selectable(true);
    text.set_justify(gtk::Justification::Left);
    text.set_line_wrap(true);
    text.set_line_wrap_mode(pango::WrapMode::Char);

    horizontal_layout.add(&label);
    horizontal_layout.add(&text);
    scroll.add(&horizontal_layout);
    text
}

fn compute_running_since(
    process: &sysinfo::Process,
    start_time: u64,
    running_since: u64,
) -> u64 {
    if start_time > process.start_time() {
        start_time - process.start_time() + running_since
    } else {
        start_time + running_since - process.start_time()
    }
}

pub fn create_process_dialog(
    process: &sysinfo::Process,
    window: &gtk::ApplicationWindow,
    running_since: u64,
    start_time: u64,
    total_memory: u64,
) -> ProcDialog {
    let mut notebook = NoteBook::new();

    let flags = gtk::DialogFlags::DESTROY_WITH_PARENT | gtk::DialogFlags::USE_HEADER_BAR;
    let popup = gtk::Dialog::new_with_buttons(
                    format!("Information about {}", process.name()).as_str(),
                    Some(window),
                    flags,
                    &[]);

    //
    // PROCESS INFO TAB
    //
    let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    let close_button = gtk::Button::new_with_label("Close");
    let vertical_layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);

    let running_since = compute_running_since(process, start_time, running_since);

    let labels = gtk::Box::new(gtk::Orientation::Vertical, 0);

    create_and_add_new_label(&labels, "name", process.name());
    create_and_add_new_label(&labels, "pid", &process.pid().to_string());
    let memory_usage = create_and_add_new_label(&labels,
                                                "memory usage",
                                                &format_number(process.memory() << 10));
    let cpu_usage = create_and_add_new_label(&labels,
                                             "cpu usage",
                                             &format!("{:.1}%", process.cpu_usage()));
    let run_time = create_and_add_new_label(&labels,
                                            "Running since",
                                            &format_time(running_since));
    create_and_add_new_label(&labels, "command", &format!("{:?}", process.cmd()));
    create_and_add_new_label(&labels, "executable path", &process.exe().display().to_string());
    let working_directory = create_and_add_new_label(&labels, "current working directory",
                                                     &process.cwd().display().to_string());
    create_and_add_new_label(&labels, "root directory", &process.root().display().to_string());
    let mut text = String::with_capacity(100);
    for env in process.environ() {
        text.push_str(&format!("\n{:?}", env));
    }
    create_and_add_new_label(&labels, "environment", &text);

    scroll.add(&labels);

    vertical_layout.pack_start(&scroll, true, true, 0);
    vertical_layout.pack_start(&close_button, false, true, 0);

    notebook.create_tab("Information", &vertical_layout);

    //
    // GRAPH TAB
    //
    let vertical_layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vertical_layout.set_spacing(5);
    vertical_layout.set_margin_top(10);
    vertical_layout.set_margin_bottom(10);
    vertical_layout.set_margin_start(5);
    vertical_layout.set_margin_end(5);
    let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    let mut cpu_usage_history = Graph::new(Some(100.), false);
    let mut ram_usage_history = Graph::new(Some(total_memory as f64), true);

    cpu_usage_history.set_display_labels(false);
    ram_usage_history.set_display_labels(false);

    cpu_usage_history.push(RotateVec::new(iter::repeat(0f64).take(61).collect()),
                           "", None);
    cpu_usage_history.set_label_callbacks(Some(Box::new(|_| {
        ["100".to_string(), "50".to_string(), "0".to_string(), "%".to_string()]
    })));
    vertical_layout.add(&gtk::Label::new("Process usage"));
    cpu_usage_history.attach_to(&vertical_layout);
    cpu_usage_history.invalidate();
    let cpu_usage_history = connect_graph(cpu_usage_history);

    ram_usage_history.push(RotateVec::new(iter::repeat(0f64).take(61).collect()),
                           "", None);
    ram_usage_history.set_label_callbacks(Some(Box::new(|v| {
        if v < 100_000. {
            [v.to_string(),
             format!("{}", v / 2.),
             "0".to_string(),
             "kB".to_string()]
        } else if v < 10_000_000. {
            [format!("{:.1}", v / 1_024f64),
             format!("{:.1}", v / 2_048f64),
             "0".to_string(),
             "MB".to_string()]
        } else if v < 10_000_000_000. {
            [format!("{:.1}", v / 1_048_576f64),
             format!("{:.1}", v / 2_097_152f64),
             "0".to_string(),
             "GB".to_string()]
        } else {
            [format!("{:.1}", v / 1_073_741_824f64),
             format!("{:.1}", v / 1_073_741_824f64),
             "0".to_string(),
             "TB".to_string()]
        }
    })));
    vertical_layout.add(&gtk::Label::new("Memory usage"));
    ram_usage_history.attach_to(&vertical_layout);
    ram_usage_history.invalidate();
    let ram_usage_history = connect_graph(ram_usage_history);

    scroll.add(&vertical_layout);
    scroll.connect_show(clone!(ram_usage_history, cpu_usage_history => move |_| {
        ram_usage_history.borrow().show_all();
        cpu_usage_history.borrow().show_all();
    }));
    notebook.create_tab("Resources usage", &scroll);

    let area = popup.get_content_area();
    area.set_margin_top(0);
    area.set_margin_bottom(0);
    area.set_margin_start(0);
    area.set_margin_end(0);
    area.pack_start(&notebook.notebook, true, true, 0);
    // To silence the annoying warning:
    // "(.:2257): Gtk-WARNING **: Allocating size to GtkWindow 0x7f8a31038290 without
    // calling gtk_widget_get_preferred_width/height(). How does the code know the size to
    // allocate?"
    popup.get_preferred_width();
    popup.set_size_request(500, 600);

    let popup = popup.upcast::<gtk::Window>();
    popup.set_resizable(true);
    popup.show_all();
    let pop = popup.clone();
    close_button.connect_clicked(move |_| {
        pop.destroy();
    });

    if let Some(adjust) = scroll.get_vadjustment() {
        adjust.set_value(0.);
        scroll.set_vadjustment(&adjust);
    }
    ram_usage_history.connect_to_window_events();
    cpu_usage_history.connect_to_window_events();

    ProcDialog {
        working_directory,
        memory_usage,
        cpu_usage,
        run_time,
        popup,
        pid: process.pid(),
        notebook,
        ram_usage_history,
        cpu_usage_history,
    }
}
