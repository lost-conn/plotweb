use crate::store::{AppStore, Route};
use rinch_core::use_store;

pub fn navigate(route: Route) {
    let store = use_store::<AppStore>();
    store.current_route.set(route);
}
