use leptos::*;

#[component]
pub fn App(cx: Scope) -> impl IntoView {
    let (count, set_count) = create_signal(cx, 0);

    view! { cx,

        <div class="flex flex-col m-2 p-2">
            <label for="peer">Remote MultiAddress:</label>
            <input type="text" id="peer" class="border border-neutral-300 bg-blue-50 rounded p-2 m-2" />
            <button id="connect" class="border bg-slate-100 rounded shadow p-2 m-2">Dial</button>
        </div>

        <button class="border bg-slate-100 rounded p-2 m-2"
            on:click=move |_| {
                set_count.update(|n| *n += 1);
            }
        >
            "Click me: "
            {move || count.get()}
        </button>
    }
}
