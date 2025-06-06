use poem::{Endpoint, EndpointExt, Route};

use crate::middleware::authorization::StaffOnly;

pub fn routes() -> impl Endpoint {
    Route::new().with(StaffOnly)
}
