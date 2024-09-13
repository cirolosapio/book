extern crate trpl; // required for mdbook test

use trpl::Html;

// ANCHOR: async-main
async fn main() {
    // ANCHOR_END: async-main
    let args: Vec<String> = std::env::args().collect();

    let url = &args[1];
    match page_title(url).await {
        Some(title) => println!("The title for {url} was {title}"),
        None => println!("{url} had no title"),
    }
}

async fn page_title(url: &str) -> Option<String> {
    let response_text = trpl::get(url).await.text().await;
    Html::parse(&response_text)
        .select_first("title")
        .map(|title_element| title_element.inner_html())
}
