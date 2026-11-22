//! Sidebar: project switcher, Overview, Add casino, and the casino list.

use super::*;

impl App {
    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        self.project_switcher(ui);
        ui.add_space(12.0);

        if theme::list_item(ui, "Overview", self.view == View::Overview, None, None).clicked() {
            self.view = View::Overview;
        }
        ui.add_space(8.0);
        if theme::list_item(ui, "Taxes", self.view == View::Taxes, None, None).clicked() {
            self.view = View::Taxes;
        }
        ui.add_space(10.0);
        theme::section(ui, "Casinos");

        let full = Vec2::new(ui.available_width() - theme::SHADOW, 0.0);
        let r = theme::shadowed(ui, |ui| {
            ui.add(egui::Button::new("+  Add casino").min_size(full))
        })
        .on_hover_cursor(egui::CursorIcon::PointingHand);
        if r.clicked() {
            self.picker.open = !self.picker.open;
            self.picker.query.clear();
            self.picker.focus = true;
        }
        self.picker.anchor = r.rect;
        ui.add_space(10.0);

        // "Recently deleted" pinned to the bottom of the sidebar, under the casino list.
        egui::TopBottomPanel::bottom("sidebar_bottom")
            .show_separator_line(false)
            .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(0.0, 10.0)))
            .show_inside(ui, |ui| {
                let count = self.deleted.casinos.len() + self.deleted.entries.len();
                let trailing = (count > 0).then(|| (count.to_string(), DIM));
                let selected = self.view == View::Trash;
                if theme::list_item(ui, "Recently deleted", selected, None, trailing).clicked() {
                    self.view = View::Trash;
                }
            });

        // Rows are button-style cards now, so the list doesn't need its own box around it.
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                {
                    for casino in &self.data.casinos {
                        let selected = self.view == View::Casino(casino.id);
                        let trailing = (casino.entry_count() > 0).then(|| {
                            let net = casino.stats().net();
                            (signed_money(net), profit_color(net))
                        });
                        let logo = self.grail.find(&casino.name).map(|g| g.logo.as_str());
                        if theme::list_item(ui, &casino.name, selected, logo, trailing).clicked() {
                            self.view = View::Casino(casino.id);
                        }
                    }
                }
            });
    }
}
