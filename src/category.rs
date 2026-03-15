//! Category resolution for apps and websites (aligned with Edge Function / CategoryService).

pub fn get_category_for_app(app_name: &str) -> String {
    if app_name.is_empty() {
        return "Uncategorized".to_string();
    }
    let normalized = app_name.to_lowercase().trim().to_string();
    let normalized = normalized.replace(' ', "");

    let app_category_map: std::collections::HashMap<&str, &str> = [
        ("code", "Development"),
        ("cursor", "Development"),
        ("windowsterminal", "Development"),
        ("notepad", "Development"),
        ("jetbrains-toolbox", "Development"),
        ("githubdesktop", "Development"),
        ("powershell", "Development"),
        ("visualstudiocode", "Development"),
        ("vscode", "Development"),
        ("ticktick", "Productivity"),
        ("excel", "Productivity"),
        ("powerpnt", "Productivity"),
        ("powerpoint", "Productivity"),
        ("word", "Productivity"),
        ("onenote", "Productivity"),
        ("outlook", "Productivity"),
        ("notes", "Productivity"),
        ("reminders", "Productivity"),
        ("calendar", "Productivity"),
        ("gemini", "Productivity"),
        ("chatgpt", "Productivity"),
        ("snippingtool", "Utilities"),
        ("winrar", "Utilities"),
        ("calculator", "Utilities"),
        ("vlc", "Entertainment"),
        ("applemusic", "Entertainment"),
        ("itunes", "Entertainment"),
        ("music", "Entertainment"),
        ("youtube", "Entertainment"),
        ("tiktok", "Entertainment"),
        ("instagram", "Entertainment"),
        ("netflix", "Entertainment"),
        ("spotify", "Entertainment"),
        ("whatsapp", "Communication"),
        ("messages", "Communication"),
        ("mail", "Communication"),
        ("telegram", "Communication"),
        ("signal", "Communication"),
        ("discord", "Communication"),
        ("slack", "Communication"),
        ("safari", "Web Browsing"),
        ("msedge", "Web Browsing"),
        ("chrome", "Web Browsing"),
        ("firefox", "Web Browsing"),
        ("explorer", "Web Browsing"),
        ("facebook", "Social"),
        ("twitter", "Social"),
        ("x", "Social"),
        ("linkedin", "Social"),
        ("reddit", "Social"),
        ("snapchat", "Social"),
        ("photoshop", "Media"),
        ("photos", "Media"),
        ("settings", "System"),
        ("clock", "System"),
        ("applicationframehost", "System"),
        ("shellhost", "System"),
        ("searchhost", "System"),
        ("credentialuibroker", "System"),
        ("lockapp", "System"),
        ("chronos-screentime", "System"),
        ("lifeos", "System"),
        ("onedrive", "Cloud"),
        ("steam", "Gaming"),
        ("epicgames", "Gaming"),
    ]
    .into_iter()
    .collect();

    if let Some(&cat) = app_category_map.get(normalized.as_str()) {
        return cat.to_string();
    }
    if normalized.contains("code") || normalized.contains("editor") || normalized.contains("ide") || normalized.contains("studio") || normalized.contains("dev") {
        return "Development".to_string();
    }
    if normalized.contains("terminal") || normalized.contains("cmd") || normalized.contains("powershell") || normalized.contains("bash") || normalized.contains("shell") {
        return "Development".to_string();
    }
    if normalized.contains("git") || normalized.contains("github") {
        return "Development".to_string();
    }
    if normalized.contains("browser") || normalized.contains("chrome") || normalized.contains("edge") || normalized.contains("firefox") || normalized.contains("safari") || normalized.contains("web") {
        return "Web Browsing".to_string();
    }
    if normalized.contains("photo") || normalized.contains("image") || normalized.contains("camera") {
        return "Media".to_string();
    }
    if normalized.contains("video") || normalized.contains("movie") || normalized.contains("vlc") {
        return "Media".to_string();
    }
    if normalized.contains("music") || normalized.contains("spotify") || normalized.contains("streaming") {
        return "Entertainment".to_string();
    }
    if normalized.contains("message") || normalized.contains("chat") || normalized.contains("whatsapp") || normalized.contains("telegram") {
        return "Communication".to_string();
    }
    if normalized.contains("mail") || normalized.contains("email") || normalized.contains("gmail") {
        return "Communication".to_string();
    }
    if normalized.contains("social") || normalized.contains("facebook") || normalized.contains("twitter") || normalized.contains("instagram") || normalized.contains("linkedin") {
        return "Social".to_string();
    }
    if normalized.contains("note") || normalized.contains("memo") || normalized.contains("notepad") || normalized.contains("document") {
        return "Productivity".to_string();
    }
    if normalized.contains("calendar") || normalized.contains("schedule") || normalized.contains("reminder") || normalized.contains("todo") || normalized.contains("task") {
        return "Productivity".to_string();
    }
    if normalized.contains("bank") || normalized.contains("finance") || normalized.contains("payment") || normalized.contains("wallet") {
        return "Finance".to_string();
    }
    if normalized.contains("game") || normalized.contains("gaming") || normalized.contains("steam") || normalized.contains("epic") {
        return "Gaming".to_string();
    }
    if normalized.contains("setting") || normalized.contains("config") || normalized.contains("preference") || normalized.contains("controlpanel") {
        return "System".to_string();
    }
    if normalized.contains("system") || normalized.contains("windows") || normalized.contains("host") || normalized.contains("service") || normalized.contains("process") {
        return "System".to_string();
    }
    if normalized.contains("cloud") || normalized.contains("sync") || normalized.contains("backup") || normalized.contains("drive") || normalized.contains("dropbox") {
        return "Cloud".to_string();
    }
    if normalized.contains("ai") || normalized.contains("assistant") || normalized.contains("chatgpt") || normalized.contains("gemini") || normalized.contains("claude") {
        return "Productivity".to_string();
    }
    "Uncategorized".to_string()
}

pub fn get_category_for_website(domain: &str) -> String {
    if domain.is_empty() {
        return "Uncategorized".to_string();
    }
    let d = domain.to_lowercase();
    if d.contains("youtube.com") || d.contains("netflix") || d.contains("twitch") || d.contains("tiktok") {
        return "Entertainment".to_string();
    }
    if d.contains("facebook") || d.contains("twitter") || d.contains("instagram") || d.contains("linkedin") || d.contains("reddit") || d.contains("snapchat") {
        return "Social".to_string();
    }
    if d.contains("github") || d.contains("gitlab") || d.contains("stackoverflow") || d.contains("bitbucket") {
        return "Development".to_string();
    }
    if d.contains("gmail") || d.contains("outlook") || d.contains("mail.") || d.contains("yahoo") {
        return "Communication".to_string();
    }
    if d.contains("drive.google") || d.contains("dropbox") || d.contains("onedrive") || d.contains("icloud") {
        return "Cloud".to_string();
    }
    if d.contains("docs.") || d.contains("notion") || d.contains("trello") || d.contains("asana") {
        return "Productivity".to_string();
    }
    "Uncategorized".to_string()
}
