# Unreleased

- Allow users to opt out of the `NetworkBehaviourEventProcess` 
mechanism through `#[behaviour(event_process = false)]`. This is
useful if users want to process all events while polling the
swarm through `SwarmEvent::Behaviour`.
