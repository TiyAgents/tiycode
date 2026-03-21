#[macro_export]
macro_rules! desktop_agent_max_turns {
    () => {
        256usize
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_agent_max_turns_defaults_to_256() {
        assert_eq!(crate::desktop_agent_max_turns!(), 256);
    }
}
