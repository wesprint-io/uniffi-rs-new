use std::sync::Arc;

#[derive(uniffi::Record)]
pub struct Id {
    value: u32,
}

#[derive(uniffi::Object)]
pub struct IdWrapper {
    pub id: Id,
}

#[derive(uniffi::Record)]
pub struct IdWrapperContainer {
    wrapper: Arc<IdWrapper>,
}

uniffi::setup_scaffolding!();
