//! Automates du catalogue étendu : groupe « carry ».

use crate::ctx::Ctx;
use crate::detail::Out;

#[derive(Clone, Debug, Default)]
pub(crate) struct Group {}

impl Group {
    #[inline]
    pub fn tick(&mut self, _cx: &Ctx, _out: &mut Out) {}
}
