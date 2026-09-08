//! Read `@grab-*` options straight from the tmux server.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::render::Styles;
use crate::tmux::Tmux;

pub const OPTION_PREFIX: &str = "@grab-";

/// Keys that cannot be hints because tmux cannot tell their Ctrl form apart
/// from another key (C-i = Tab, C-m = Enter) or because they exit grab mode.
pub const RESERVED_KEYS: &[char] = &['c', 'i', 'm', 'q'];

pub const BUILTIN_PATTERNS: &[(&str, &str)] = &[
    ("ip", r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}"),
    (
        "uuid",
        r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
    ),
    ("sha", r"[0-9a-f]{7,128}"),
    ("digit", r"[0-9]{4,}"),
    (
        "url",
        r#"((https?://|git@|git://|ssh://|ftp://|file:///)[^\s()"']+)"#,
    ),
    ("path", r"(([.\w\-~\$@]+)?(/[.\w\-@]+)+/?)"),
    ("hex", r"(0x[0-9a-fA-F]+)"),
    (
        "kubernetes",
        r"(deployment.app|binding|componentstatuse|configmap|endpoint|event|limitrange|namespace|node|persistentvolumeclaim|persistentvolume|pod|podtemplate|replicationcontroller|resourcequota|secret|serviceaccount|service|mutatingwebhookconfiguration.admissionregistration.k8s.io|validatingwebhookconfiguration.admissionregistration.k8s.io|customresourcedefinition.apiextension.k8s.io|apiservice.apiregistration.k8s.io|controllerrevision.apps|daemonset.apps|deployment.apps|replicaset.apps|statefulset.apps|tokenreview.authentication.k8s.io|localsubjectaccessreview.authorization.k8s.io|selfsubjectaccessreviews.authorization.k8s.io|selfsubjectrulesreview.authorization.k8s.io|subjectaccessreview.authorization.k8s.io|horizontalpodautoscaler.autoscaling|cronjob.batch|job.batch|certificatesigningrequest.certificates.k8s.io|events.events.k8s.io|daemonset.extensions|deployment.extensions|ingress.extensions|networkpolicies.extensions|podsecuritypolicies.extensions|replicaset.extensions|networkpolicie.networking.k8s.io|poddisruptionbudget.policy|clusterrolebinding.rbac.authorization.k8s.io|clusterrole.rbac.authorization.k8s.io|rolebinding.rbac.authorization.k8s.io|role.rbac.authorization.k8s.io|storageclasse.storage.k8s.io)[[:alnum:]_#$%&+=/@-]+",
    ),
    (
        "git-status",
        r"(modified|deleted|deleted by us|new file): +(?P<match>.+)",
    ),
    (
        "git-status-branch",
        r"Your branch is up to date with '(?P<match>.*)'\.",
    ),
    ("diff", r"(---|\+\+\+) [ab]/(?P<match>.*)"),
];

pub const ALPHABETS: &[(&str, &str)] = &[
    ("qwerty", "asdfqwerzxcvjklmiuopghtybn"),
    ("qwerty-homerow", "asdfjklgh"),
    ("qwerty-left-hand", "asdfqwerzcxv"),
    ("qwerty-right-hand", "jkluiopmyhn"),
    ("azerty", "qsdfazerwxcvjklmuiopghtybn"),
    ("azerty-homerow", "qsdfjkmgh"),
    ("azerty-left-hand", "qsdfazerwxcv"),
    ("azerty-right-hand", "jklmuiophyn"),
    ("qwertz", "asdfqweryxcvjkluiopmghtzbn"),
    ("qwertz-homerow", "asdfghjkl"),
    ("qwertz-left-hand", "asdfqweryxcv"),
    ("qwertz-right-hand", "jkluiopmhzn"),
    ("dvorak", "aoeuqjkxpyhtnsgcrlmwvzfidb"),
    ("dvorak-homerow", "aoeuhtnsid"),
    ("dvorak-left-hand", "aoeupqjkyix"),
    ("dvorak-right-hand", "htnsgcrlmwvz"),
    ("colemak", "arstqwfpzxcvneioluymdhgjbk"),
    ("colemak-homerow", "arstneiodh"),
    ("colemak-left-hand", "arstqwfpzxcv"),
    ("colemak-right-hand", "neioluymjhk"),
];

#[derive(Debug, Clone)]
pub struct Config {
    pub key: String,
    pub keyboard_layout: String,
    pub alphabet: Vec<char>,
    /// (name, regex) in priority order: user patterns first, then builtins.
    pub patterns: Vec<(String, String)>,
    pub main_action: String,
    pub ctrl_action: String,
    pub shift_action: String,
    pub alt_action: String,
    pub use_system_clipboard: bool,
    pub show_copied_notification: bool,
    pub enable_bindings: bool,
    pub styles: Styles,
    /// Raw style strings, kept for error reporting.
    pub raw_styles: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        let mut c = Self {
            key: "F".into(),
            keyboard_layout: "qwerty".into(),
            alphabet: Vec::new(),
            patterns: builtin_pattern_pairs(&["all".to_string()]).unwrap(),
            main_action: ":copy:".into(),
            ctrl_action: ":open:".into(),
            shift_action: ":paste:".into(),
            alt_action: String::new(),
            use_system_clipboard: true,
            show_copied_notification: false,
            enable_bindings: true,
            styles: Styles {
                hint: crate::style::to_ansi("fg=green,bold").unwrap(),
                highlight: crate::style::to_ansi("fg=yellow").unwrap(),
                selected_hint: crate::style::to_ansi("fg=blue,bold").unwrap(),
                selected_highlight: crate::style::to_ansi("fg=blue").unwrap(),
                backdrop: String::new(),
                hint_on_right: false,
            },
            raw_styles: BTreeMap::new(),
        };
        c.alphabet = alphabet_for("qwerty").unwrap();
        c
    }
}

pub fn alphabet_for(layout: &str) -> Result<Vec<char>> {
    let Some((_, letters)) = ALPHABETS.iter().find(|(name, _)| *name == layout) else {
        bail!(
            "unknown keyboard layout '{layout}' (valid: {})",
            ALPHABETS
                .iter()
                .map(|(n, _)| *n)
                .collect::<Vec<_>>()
                .join(", ")
        );
    };
    Ok(letters
        .chars()
        .filter(|c| !RESERVED_KEYS.contains(c))
        .collect())
}

/// Resolve `all` or a comma list of builtin names to (name, regex) pairs.
pub fn builtin_pattern_pairs(names: &[String]) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for name in names.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if name == "all" {
            out.extend(
                BUILTIN_PATTERNS
                    .iter()
                    .map(|(n, p)| (n.to_string(), p.to_string())),
            );
            continue;
        }
        match BUILTIN_PATTERNS.iter().find(|(n, _)| *n == name) {
            Some((n, p)) => out.push((n.to_string(), p.to_string())),
            None => bail!(
                "unknown builtin pattern '{name}' (valid: all, {})",
                BUILTIN_PATTERNS
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
    Ok(out)
}

/// Regex strings only, for `all` or a comma list of builtin names.
#[cfg(test)]
pub fn builtin_patterns(names: &[String]) -> Result<Vec<String>> {
    Ok(builtin_pattern_pairs(names)?
        .into_iter()
        .map(|(_, p)| p)
        .collect())
}

impl Config {
    /// Read every `@grab-*` option from the running tmux server. Collects all
    /// problems instead of stopping at the first so load-config can list them.
    pub fn load(tmux: &Tmux) -> Result<Config, Vec<String>> {
        let raw = match tmux.grab_options() {
            Ok(r) => r,
            Err(e) => return Err(vec![format!("cannot read tmux options: {e}")]),
        };
        Self::from_options(&raw)
    }

    pub fn from_options(raw: &BTreeMap<String, String>) -> Result<Config, Vec<String>> {
        let mut cfg = Config::default();
        let mut errors = Vec::new();
        let mut user_patterns: Vec<(String, String)> = Vec::new();
        let mut builtin_names = vec!["all".to_string()];

        for (full_name, value) in raw {
            let Some(name) = full_name.strip_prefix(OPTION_PREFIX) else {
                continue;
            };
            let value = value.as_str();
            match name {
                "key" => cfg.key = value.to_string(),
                "keyboard-layout" => cfg.keyboard_layout = value.to_string(),
                "main-action" => cfg.main_action = value.to_string(),
                "ctrl-action" => cfg.ctrl_action = value.to_string(),
                "shift-action" => cfg.shift_action = value.to_string(),
                "alt-action" => cfg.alt_action = value.to_string(),
                "use-system-clipboard" => cfg.use_system_clipboard = truthy(value),
                "show-copied-notification" => cfg.show_copied_notification = truthy(value),
                "enable-bindings" => cfg.enable_bindings = truthy(value),
                "hint-position" => match value {
                    "left" => cfg.styles.hint_on_right = false,
                    "right" => cfg.styles.hint_on_right = true,
                    _ => errors.push(format!(
                        "{full_name}: expected 'left' or 'right', got '{value}'"
                    )),
                },
                "hint-style"
                | "highlight-style"
                | "selected-hint-style"
                | "selected-highlight-style"
                | "backdrop-style" => {
                    cfg.raw_styles.insert(name.to_string(), value.to_string());
                    match crate::style::to_ansi(value) {
                        Ok(seq) => match name {
                            "hint-style" => cfg.styles.hint = seq,
                            "highlight-style" => cfg.styles.highlight = seq,
                            "selected-hint-style" => cfg.styles.selected_hint = seq,
                            "selected-highlight-style" => cfg.styles.selected_highlight = seq,
                            _ => cfg.styles.backdrop = seq,
                        },
                        Err(e) => errors.push(format!("{full_name}: {e}")),
                    }
                }
                "enabled-builtin-patterns" => {
                    builtin_names = value.split(',').map(|s| s.trim().to_string()).collect();
                }
                "skip-wizard" | "cli" => {}
                _ => {
                    if let Some(pname) = name.strip_prefix("pattern-") {
                        if value.is_empty() {
                            continue;
                        }
                        if let Err(e) = regex::Regex::new(value) {
                            errors.push(format!("{full_name}: invalid regex: {e}"));
                        } else {
                            user_patterns.push((pname.to_string(), value.to_string()));
                        }
                    } else {
                        errors.push(format!("'{full_name}' is not a valid tmux-grab option"));
                    }
                }
            }
        }

        match alphabet_for(&cfg.keyboard_layout) {
            Ok(a) => cfg.alphabet = a,
            Err(e) => errors.push(format!("{OPTION_PREFIX}keyboard-layout: {e}")),
        }
        match builtin_pattern_pairs(&builtin_names) {
            Ok(b) => {
                cfg.patterns = user_patterns;
                cfg.patterns.extend(b);
            }
            Err(e) => errors.push(format!("{OPTION_PREFIX}enabled-builtin-patterns: {e}")),
        }

        if errors.is_empty() {
            Ok(cfg)
        } else {
            Err(errors)
        }
    }

    /// Regexes for the given pattern names, or all configured patterns.
    pub fn pattern_regexes(&self, only: Option<&str>) -> Result<Vec<String>> {
        let Some(only) = only else {
            return Ok(self.patterns.iter().map(|(_, p)| p.clone()).collect());
        };
        let mut out = Vec::new();
        for name in only.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match self.patterns.iter().find(|(n, _)| n == name) {
                Some((_, p)) => out.push(p.clone()),
                None => bail!("unknown pattern '{name}'"),
            }
        }
        Ok(out)
    }

    pub fn action_for(&self, modifier: &str) -> &str {
        match modifier {
            "ctrl" => &self.ctrl_action,
            "shift" => &self.shift_action,
            "alt" => &self.alt_action,
            _ => &self.main_action,
        }
    }
}

fn truthy(v: &str) -> bool {
    matches!(v.trim(), "1" | "on" | "true" | "yes")
}

/// Directory for the input socket.
pub fn runtime_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR") {
        let p = PathBuf::from(d);
        if p.is_dir() {
            return p;
        }
    }
    cache_dir()
}

/// Directory for the log file.
pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("tmux-grab")
}

pub fn log_path() -> PathBuf {
    cache_dir().join("tmux-grab.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn defaults_when_nothing_set() {
        let c = Config::from_options(&BTreeMap::new()).unwrap();
        assert_eq!(c.key, "F");
        assert_eq!(c.patterns.len(), BUILTIN_PATTERNS.len());
        assert!(!c.alphabet.contains(&'q'));
        assert!(c.alphabet.contains(&'a'));
    }

    #[test]
    fn user_options_override() {
        let c = Config::from_options(&opts(&[
            ("@grab-key", "f"),
            ("@grab-hint-style", "bg=yellow,fg=black,bold"),
            ("@grab-enabled-builtin-patterns", "url,path"),
            ("@grab-pattern-jira", "[A-Z]{2,}-[0-9]+"),
            ("@grab-use-system-clipboard", "0"),
            ("@grab-hint-position", "right"),
            ("@grab-keyboard-layout", "qwerty-homerow"),
        ]))
        .unwrap();
        assert_eq!(c.key, "f");
        assert_eq!(c.styles.hint, "\x1b[43m\x1b[30m\x1b[1m");
        assert_eq!(
            c.patterns
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>(),
            vec!["jira", "url", "path"]
        );
        assert!(!c.use_system_clipboard);
        assert!(c.styles.hint_on_right);
        assert_eq!(c.alphabet, "asdfjklgh".chars().collect::<Vec<_>>());
        assert_eq!(c.pattern_regexes(Some("url")).unwrap().len(), 1);
        assert!(c.pattern_regexes(Some("nope")).is_err());
    }

    #[test]
    fn collects_all_errors() {
        let errs = Config::from_options(&opts(&[
            ("@grab-bogus", "1"),
            ("@grab-hint-style", "fg=purple"),
            ("@grab-pattern-bad", "("),
            ("@grab-keyboard-layout", "klingon"),
            ("@grab-enabled-builtin-patterns", "url,nope"),
            ("@grab-hint-position", "middle"),
        ]))
        .unwrap_err();
        assert_eq!(errs.len(), 6, "{errs:#?}");
    }

    #[test]
    fn all_builtin_patterns_compile() {
        for (name, p) in BUILTIN_PATTERNS {
            regex::Regex::new(p).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}
