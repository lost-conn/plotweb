use std::sync::Arc;

#[derive(Clone)]
pub struct EmailService {
    client: reqwest::Client,
    api_key: String,
    from: String,
    app_url: String,
}

impl EmailService {
    /// Create an EmailService from environment variables.
    /// Returns None if RESEND_API_KEY is not set (notifications disabled).
    pub fn from_env() -> Option<Arc<Self>> {
        let api_key = std::env::var("RESEND_API_KEY").ok()?;
        if api_key.is_empty() {
            return None;
        }
        Some(Arc::new(Self {
            client: reqwest::Client::new(),
            api_key,
            from: std::env::var("RESEND_FROM")
                .unwrap_or_else(|_| "PlotWeb <notifications@plotweb.app>".into()),
            app_url: std::env::var("APP_URL")
                .unwrap_or_else(|_| "http://localhost:8080".into())
                .trim_end_matches('/')
                .to_string(),
        }))
    }

    async fn send_email(&self, to: &str, subject: &str, html: &str) {
        let res = self
            .client
            .post("https://api.resend.com/emails")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "from": self.from,
                "to": [to],
                "subject": subject,
                "html": html,
            }))
            .send()
            .await;

        match res {
            Ok(resp) if !resp.status().is_success() => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                eprintln!("Resend API error ({}): {}", status, body);
            }
            Err(e) => eprintln!("Failed to send email: {}", e),
            _ => {}
        }
    }

    /// Notify the book author that a reader left new feedback.
    pub async fn notify_new_feedback(
        &self,
        to: &str,
        book_title: &str,
        chapter_title: &str,
        reader_name: &str,
        comment: &str,
        book_id: &str,
    ) {
        let subject = format!("New feedback on \"{}\"", book_title);
        let link = format!("{}/book/{}", self.app_url, book_id);
        let html = format!(
            r#"<p><strong>{reader_name}</strong> left feedback on chapter "<strong>{chapter_title}</strong>" in <em>{book_title}</em>:</p>
<blockquote style="border-left: 3px solid #ccc; padding-left: 12px; color: #555;">{comment}</blockquote>
<p><a href="{link}">View in PlotWeb</a></p>"#
        );
        self.send_email(to, &subject, &html).await;
    }

    /// Notify the book author that a reader replied to a feedback thread.
    pub async fn notify_reader_reply(
        &self,
        to: &str,
        book_title: &str,
        reader_name: &str,
        reply_content: &str,
        book_id: &str,
    ) {
        let subject = format!("{} replied to feedback on \"{}\"", reader_name, book_title);
        let link = format!("{}/book/{}", self.app_url, book_id);
        let html = format!(
            r#"<p><strong>{reader_name}</strong> replied to feedback on <em>{book_title}</em>:</p>
<blockquote style="border-left: 3px solid #ccc; padding-left: 12px; color: #555;">{reply_content}</blockquote>
<p><a href="{link}">View in PlotWeb</a></p>"#
        );
        self.send_email(to, &subject, &html).await;
    }

    /// Notify a beta reader that the author replied to their feedback.
    pub async fn notify_author_reply(
        &self,
        to: &str,
        book_title: &str,
        author_name: &str,
        reply_content: &str,
        token: &str,
    ) {
        let subject = format!("{} replied to your feedback on \"{}\"", author_name, book_title);
        let link = format!("{}/read/{}", self.app_url, token);
        let html = format!(
            r#"<p><strong>{author_name}</strong> replied to your feedback on <em>{book_title}</em>:</p>
<blockquote style="border-left: 3px solid #ccc; padding-left: 12px; color: #555;">{reply_content}</blockquote>
<p><a href="{link}">View in PlotWeb</a></p>"#
        );
        self.send_email(to, &subject, &html).await;
    }
}
