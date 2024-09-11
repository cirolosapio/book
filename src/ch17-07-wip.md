> ## Restructuring
>
> This is just a placeholder for material that needs to be restructured so that
> the earlier sections of the book can avoid getting sidetracked into details of
> things like `Pin` or even just the full gnarliness of the `Future` trait at
> points where it would be better for the text to keep moving.

---

Here is the definition of the trait:

```rust
use std::pin::Pin;
use std::task::{Context, Poll};

pub trait Future {
    type Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>;
}
```

As we learned earlier, `Future`’s associated type `Output` says what the future
will resolves to. (This is analogous to the `Item` associated type for the
`Iterator` trait.) Beyond that, `Future` also has the `poll` method, which takes
a special `Pin` reference for its `self` parameter and a mutable reference to a
`Context` type, and returns a `Poll<Self::Output>`. We will talk a little more
about `Pin` and `Context` later in the chapter. For now, let’s focus on what the
method returns, the `Poll` type:

```rust
enum Poll<T> {
    Ready(T),
    Pending
}
```

This `Poll` type is a lot like an `Option`: it has one variant which has a value
(`Ready(T)`), and one which does not (`Pending`). It means something quite
different, though! The `Pending` variant indicates that the future still has
work to do, so the caller will need to check again later. The `Ready` variant
indicates that the `Future` has finished its work and the `T` value is
available.

> Note: With most futures, the caller should not call `poll()` again after the
> future has returned `Ready`. Many futures will panic if polled again after
> becoming ready! Futures which are safe to poll again will say so explicitly in
> their documentation.

Under the hood, when you call `.await`, Rust compiles that to code which calls
`poll`, kind of (although not exactly <!-- TODO: describe `IntoFuture`? -->)
like this:

```rust,ignore
match hello("async").poll() {
    Ready(_) => {
        // We’re done!
    }
    Pending => {
        // But what goes here?
    }
}
```

What should we do when the `Future` is still `Pending`? We need some way to try
again… and again, and again, until the future is finally ready. In other words,
a loop:

```rust,ignore
let hello_fut = hello("async");
loop {
    match hello_fut.poll() {
        Ready(_) => {
            break;
        }
        Pending => {
            // continue
        }
    }
}
```

If Rust compiled it to exactly that code, though, every `.await` would be
blocking—exactly the opposite of what we were going for! Instead, Rust needs
makes sure that the loop can hand off control to something which can pause work
on this future and work on other futures and check this one again later. That
“something” is an async runtime, and this scheduling and coordination work is
one of the main jobs for a runtime.

---

> Note: If you want to understand how things work “under the hood,” the official
> [_Asynchronous Programming in Rust_][async-book] book covers them:
>
> - [Chapter 2: Under the Hood: Executing Futures and Tasks][under-the-hood]
> - [Chapter 4: Pinning][pinning].

---

Recall our description of how `rx.recv()` waits in the [Counting][counting]
section. The `recv()` call returns a `Future`, and awaiting it polls it. In our
initial discussion, we noted that a runtime will pause the future until it is
ready with either `Some(message)` or `None` when the channel closes. With a
deeper understanding of `Future` in place, and specifically its `poll` method,
we can see how that works. The runtime knows the future is not ready when it
returns `Poll::Pending`. Conversely, the runtime knows the future is ready and
advances it when `poll` returns `Poll::Ready(Some(message))` or
`Poll::Ready(None)`.

[counting]: /ch17-02-concurrency-with-async.md

---

<!--
    From my own notes when rereading the chapter:

    > Incoherent sentence and a *lot* of material in it which we do not unpack in this paragraph.
 -->

The rest of the message tells us *why* that is required: the `JoinAll`
struct returned by `trpl::join_all` is generic over a type `F` which must
implement the `Future` trait, directly awaiting a Future requires that the
future implement the `Unpin` trait. Understanding this error means we need to
dive into a little more of how the `Future` type actually works, in particular
the idea of *pinning*.

### Pinning and the Pin and Unpin Traits

<!-- TODO: get a *very* careful technical review of this section! -->

Let’s look again at the definition of `Future`, focusing now on its `poll`
method’s `self` type:

```rust
use std::pin::Pin;
use std::task::{Context, Poll};

pub trait Future {
    type Output;

    // Required method
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>;
}
```

This is the first time we have seen a method where `self` has a type annotation
like this. When we specify the type of `self` like this, we are telling Rust
what type `self` must be to call this method. These kinds of type annotations
for `self` are similar to those for other function parameters, but with the
restriction that the type annotation has to be the type on which the method is
implemented, or a reference or smart pointer to that type. We will see more on
this syntax in Chapter 18. For now, it is enough to know that if we want to poll
a future (to check whether it is `Pending` or `Ready(Output)`), we need a
mutable reference to the type, which is wrapped in a `Pin`.

`Pin` is a smart pointer, much like `Box`, `Rc`, and the others we saw in
Chapter 15. Unlike those, however, `Pin` only works with *other pointer types*
like reference (`&` and `&mut`) and smart pointers (`Box`, `Rc`, and so on). To
be precise, `Pin` works with types which implement the `Deref` or `DerefMut`
traits, which we covered in Chapter 15. You can think of this restriction as
equivalent to only working with pointers, though, since implementing `Deref` or
`DerefMut` means your type behaves like a pointer type.

Recalling that `.await` is implemented in terms of calls to `poll()`, this
starts to explain the error message we saw above—but that was in terms of
`Unpin`, not `Pin`. So what exactly are `Pin` and `Unpin`, how do they relate,
and why does `Future` need `self` to be in a `Pin` type to call `poll`?

In [“What Are Futures”][what-are-futures], we described how a series of await
points in a future get compiled into a state machine—and noted how the compiler
helps make sure that state machine follows all of Rust’s normal rules around
safety, including borrowing and ownership. To make that work, Rust looks at what
data is needed between each await point and the next await point or the end of
the async block. It then creates a corresponding variant in the state machine it
creates. Each variant gets the access it needs to the data that will be used in
that section of the source code, whether by taking ownership of that data or by
getting a mutable or immutable reference to it.

So far so good: if we get anything wrong about the ownership or references in a
given async block, the borrow checker will tell us. When we want to move around
the future that corresponds to that block—like moving it into a `Vec` to pass to
`join_all`—things get trickier.

When we move a future—whether by pushing into a data structure to use as an
iterator with `join_all`, or returning them from a function—that actually means
moving the state machine Rust creates for us. And unlike most other types in
Rust, the futures Rust creates for async blocks can end up with references to
themselves in the fields of any given variant. Any object which has a reference
to itself is unsafe to move, though, because references always point to the
actual memory address of the thing they refer to. If you move the data structure
itself, you *have* to update any references to it, or they will be left pointing
to the old location.

In principle, you could make the Rust compiler try to update every reference to
an object every time it gets moved. That would potentially be a lot of
performance overhead, especially given there can be a whole web of references
that need updating. On the other hand, if we could make sure the data structure
in question *does not move in memory*, we do not have to update any references.
And this is exactly what Rust’s borrow checker already guarantees: you cannot
move an item which has any active references to it using safe code.

`Pin` builds on that to give us the exact guarantee we need. When we *pin* a
value by wrapping a pointer to it in `Pin`, it can no longer move. Thus, if you
have `Pin<Box<SomeType>>`, you actually pin the `SomeType` value, *not* the
`Box` pointer. In fact, the pinned box pointer can move around freely. Remember:
we care about making sure the data ultimately being referenced stays in its
place. If a pointer moves around, but the data it points to is in the same
place, there is no problem.

However, most types are perfectly safe to move around, even if they happen to be
behind a `Pin` pointer. We only need to think about pinning when items have
internal references. Primitive values like numbers and booleans do not have any
internal structure like that, so they are obviously safe. Neither do most types
you normally work with in Rust. A `Vec`, for example, does not have any internal
references it needs to keep up to date this way, so you can move it around
without worrying. If you have a `Pin<Vec<String>>`, you would have to do
everything via Pin’s safe but restrictive APIs, even though a `Vec<String>` is
always safe to move if there are no other references to it. We need a way to
tell the compiler that it is actually just fine to move items around in cases
like these. For that, we have `Unpin`.

`Unpin` is a marker trait, like `Send` and `Sync`, which we saw in Chapter 16.
Recall that marker traits have no functionality of their own. They exist only to
tell the compiler that it is safe to use the type which implements a given trait
in a particular context. `Unpin` informs the compiler that a given type does
*not* need to uphold any particular guarantees about whether the value in
question can be moved.

Just like `Send` and `Sync`, the compiler implements `Unpin` automatically for
all types where it can prove it is safe. Implementing `Unpin` manually is unsafe
because it requires *you* to uphold all the guarantees which make `Pin` and
`Unpin` safe yourself for a type with internal references. In practice, this is
a very rare thing to implement yourself!

> Note: This combination of `Pin` and `Unpin` allows a whole class of complex
> types to be safe in Rust which are otherwise difficult to implement because
> they are self-referential. Types which require `Pin` show up *most* commonly
> in async Rust today, but you might—very rarely!—see it in other contexts, too.
>
> The specific mechanics for how `Pin` and `Unpin` work under the hood are
> covered extensively in the API documentation for `std::pin`, so if you would
> like to understand them more deeply, that is a great place to start.

Now we know enough to fix the last errors with `join_all`. We tried to move the
futures produced by an async blocks into a `Vec<Box<dyn Future<Output = ()>>>`,
but as we have seen, those futures may have internal references, so they do not
implement `Unpin`. They need to be pinned, and then we can pass the `Pin` type
into the `Vec`, confident that the underlying data in the futures will *not* be
moved.