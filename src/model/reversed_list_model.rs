use gio::prelude::*;
use gio::subclass::prelude::*;
use gtk::gio;
use gtk::glib;

mod imp {
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug, Default)]
    pub(crate) struct ReversedListModel {
        pub(super) model: RefCell<Option<gio::ListModel>>,
        pub(super) handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ReversedListModel {
        const NAME: &'static str = "ReversedListModel";
        type Type = super::ReversedListModel;
        type Interfaces = (gio::ListModel,);
    }

    impl ObjectImpl for ReversedListModel {
        fn dispose(&self) {
            if let (Some(model), Some(handler)) =
                (self.model.take(), self.handler.take())
            {
                model.disconnect(handler);
            }
        }
    }

    impl ListModelImpl for ReversedListModel {
        fn item_type(&self) -> glib::Type {
            self.model
                .borrow()
                .as_ref()
                .map(|model| model.item_type())
                .unwrap_or(glib::Object::static_type())
        }

        fn n_items(&self) -> u32 {
            self.model
                .borrow()
                .as_ref()
                .map(|model| model.n_items())
                .unwrap_or(0)
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            let model = self.model.borrow();
            let model = model.as_ref()?;
            let n_items = model.n_items();

            if position >= n_items {
                return None;
            }

            model.item(n_items - 1 - position)
        }
    }
}

glib::wrapper! {
    /// A list model that presents the items of another list model in reverse
    /// order.
    ///
    /// The chat history is kept with the newest message first, while a
    /// `GtkListView` shows the first item at the top. This model lets the
    /// history be shown with the newest message at the bottom without a
    /// patched GTK.
    pub(crate) struct ReversedListModel(ObjectSubclass<imp::ReversedListModel>)
        @implements gio::ListModel;
}

impl ReversedListModel {
    pub(crate) fn new(model: &impl IsA<gio::ListModel>) -> Self {
        let obj: Self = glib::Object::new();
        let model = model.upcast_ref::<gio::ListModel>();

        let handler = model.connect_items_changed(glib::clone!(
            #[weak]
            obj,
            move |model, position, removed, added| {
                // `n_items` is already the new length here. The changed range
                // [position, position + added) of the new list is
                // [n_items - position - added, n_items - position) in the
                // reversed one, and the same start applies to the removed
                // range of the old list.
                let n_items = model.n_items();
                obj.items_changed(n_items - position - added, removed, added);
            }
        ));

        let imp = obj.imp();
        imp.model.replace(Some(model.clone()));
        imp.handler.replace(Some(handler));

        obj
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(items: &[&'static str]) -> gio::ListStore {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        splice(&store, 0, 0, items);
        store
    }

    fn splice(store: &gio::ListStore, position: u32, removed: u32, items: &[&'static str]) {
        let items: Vec<glib::BoxedAnyObject> =
            items.iter().map(|s| glib::BoxedAnyObject::new(*s)).collect();
        store.splice(position, removed, &items);
    }

    fn strings(model: &impl IsA<gio::ListModel>) -> Vec<&'static str> {
        (0..model.n_items())
            .map(|i| {
                *model
                    .item(i)
                    .and_downcast::<glib::BoxedAnyObject>()
                    .unwrap()
                    .borrow::<&'static str>()
            })
            .collect()
    }

    #[test]
    fn mirrors_changes_of_the_inner_model() {
        let inner = store(&["c", "b", "a"]);
        let reversed = ReversedListModel::new(&inner);
        assert_eq!(strings(&reversed), ["a", "b", "c"]);

        // Replaying the emitted changes on a copy must give the same list, so
        // the positions of items-changed are right.
        let copy = store(&["a", "b", "c"]);
        reversed.connect_items_changed(glib::clone!(
            #[strong]
            copy,
            move |reversed, position, removed, added| {
                let new = strings(reversed);
                splice(
                    &copy,
                    position,
                    removed,
                    &new[position as usize..(position + added) as usize],
                );
            }
        ));

        // A new message (front of the inner model, bottom of the view)
        splice(&inner, 0, 0, &["d"]);
        // Older messages (back of the inner model, top of the view)
        splice(&inner, inner.n_items(), 0, &["z", "y"]);
        // A removal in the middle
        splice(&inner, 2, 1, &[]);
        // A replacement
        splice(&inner, 1, 2, &["x", "w", "v"]);

        let mut expected = strings(&inner);
        expected.reverse();
        assert_eq!(strings(&reversed), expected);
        assert_eq!(strings(&copy), expected);
    }
}
