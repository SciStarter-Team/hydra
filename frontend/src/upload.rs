use poem::{Endpoint, Route, handler};

pub fn authenticated_routes() -> impl Endpoint {
    Route::new()
}

pub fn anonymous_routes() -> impl Endpoint {
    Route::new()
}
