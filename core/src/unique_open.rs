

pub struct UniqueOpen<T> {
    inner: UniqueOpenInner<T>,
}

enum UniqueOpenInner<T> {
    Empty,
    Dialed {
        on_receive: oneshot::Sender<T>,
        receiver: future::Shared<oneshot::Receiver<T>>,
    },
    Received {
        receiver: future::Shared<oneshot::Receiver<T>>
    },
    Poisoned,
}

impl<T> UniqueOpen<T> {
    pub fn set_if_notset(&mut self, value: T) {
        match mem::replace(&self.inner, UniqueOpenInner::Poisoned) {
            UniqueOpenInner::Empty => {
                let (tx, rx) = oneshot::channel();
                tx.send(value);
                self.inner = UniqueOpenInner::Received { rx.shared() };
            },
            UniqueOpenInner::Dialed { on_receive, receiver } => {
                on_receive.send(value);
                self.inner = UniqueOpenInner::Received { receiver };
            }
            state @ UniqueOpenInner::Received { .. } => {
                self.inner = state;
            }
            UniqueOpenInner::Poisoned => {
                panic!("An earlier panic happened, the UniqueOpen is now corrupted")
            }
        }
    }

    pub fn get<F, Fut>(&mut self, or: F)
        where F: FnOnce() -> Fut,
              Fut: IntoFuture<Item = T, Error = IoError>
    {
        match self.inner {
            UniqueOpenInner::Empty => {
                let (tx, rx) = oneshot::channel();
                self.inner = UniqueOpenInner::Dialed {
                    on_receive: tx,
                    receiver: rx.shared(),
                };
            },
            UniqueOpenInner::Dialed { on_receive, receiver } => {
                receiver.clone()
            }
            state @ UniqueOpenInner::Received { receiver } => {
                receiver.clone()
            }
            UniqueOpenInner::Poisoned => {
                panic!("An earlier panic happened, the UniqueOpen is now corrupted")
            }
        }
    }
}

