/// Logging levers, by default all tasks log as L1, but can be changed to
/// l0, l2, l3 by using #l0 #l2 #l3 tags in the task name.
/// Reporters can be set to ignore anything up from a certain level.
#[derive(Clone, Copy, Default, PartialEq, PartialOrd, Eq, Ord)]
pub enum Level {
    L0,
    #[default]
    L1,
    L2,
    L3,
}
