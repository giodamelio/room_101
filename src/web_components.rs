use maud::{Markup, html};

// Navigation breadcrumb component
pub fn nav_breadcrumb(href: &str, text: &str) -> Markup {
    html! {
        nav style="margin-bottom: 20px;" {
            a href=(href) { "← " (text) }
        }
    }
}

// Card container component
pub fn card_container(content: Markup, _padding: Option<&str>) -> Markup {
    html! {
        div style="background: white; border: 1px solid #ddd; border-radius: 8px; padding: 20px; margin-bottom: 20px;" {
            (content)
        }
    }
}

// List item card (for peer/secret lists)
pub fn list_item_card(content: Markup) -> Markup {
    html! {
        div style="background: white; border: 1px solid #ddd; border-radius: 8px; padding: 16px; box-shadow: 0 2px 4px rgba(0,0,0,0.1);" {
            (content)
        }
    }
}

// Empty state component
pub fn empty_state(icon: &str, title: &str, description: &str) -> Markup {
    html! {
        div style="text-align: center; padding: 40px; background: white; border-radius: 8px; border: 1px solid #ddd;" {
            div style="font-size: 3em; margin-bottom: 16px; color: #999;" { (icon) }
            h3 style="margin: 0 0 8px 0; color: #666;" { (title) }
            p style="margin: 0; color: #888;" { (description) }
        }
    }
}
