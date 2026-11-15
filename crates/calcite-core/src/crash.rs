//! Crash doctor: read a crash report (or the end of a log) and say, in plain
//! words, what most likely went wrong and what would fix it. Offline and
//! rule-based; the rules come from real Fabric, Forge and NeoForge crashes.

#[derive(Clone, Debug, PartialEq)]
pub enum Cause {
    /// The game ran out of memory.
    OutOfMemory,
    /// A mod needs a newer Java than the one used.
    WrongJava {
        needs: u32,
    },
    /// Mods are missing something they need (or need a different version).
    MissingDependency {
        problems: Vec<String>,
    },
    /// A mod was built for a different Minecraft version.
    WrongMinecraftVersion,
    /// The graphics driver or GPU failed.
    Graphics,
    /// The game froze while closing; nothing was lost.
    HungWhileClosing,
    /// A specific mod failed (mixin, entrypoint, or its code in the trace).
    ModFailed,
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Diagnosis {
    pub cause: Cause,
    /// One line, plain words.
    pub title: String,
    /// What to do about it.
    pub advice: String,
    /// Mod ids most likely involved, most likely first.
    pub suspects: Vec<String>,
    /// The exception line the report led with, for the curious.
    pub raw: String,
}

/// Ids that show up in traces but are never the culprit.
const NOT_SUSPECTS: [&str; 10] = [
    "minecraft",
    "fabric",
    "fabricloader",
    "forge",
    "neoforge",
    "java",
    "mixin",
    "mixinextras",
    "fabric_api",
    "fabric-api",
];

fn push_unique(list: &mut Vec<String>, id: &str) {
    let id = id
        .trim()
        .trim_matches(['\'', '"', ']', ')'])
        .to_ascii_lowercase();
    if id.is_empty() || NOT_SUSPECTS.contains(&id.as_str()) || id.len() > 64 {
        return;
    }
    if !list.contains(&id) {
        list.push(id);
    }
}

/// Mod ids named by the report: mixin configs, entrypoints, Forge failure
/// messages, and mixin handler names in the stack trace.
pub fn suspects(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        // "Mixin [config:Class from mod iris] ... FAILED"
        if let Some(at) = line.find(" from mod ") {
            let rest = &line[at + 10..];
            push_unique(&mut found, rest.split([']', ' ']).next().unwrap_or(""));
        }
        // "... due to errors, provided by 'carpet' at ..."
        if let Some(at) = line.find("provided by '") {
            let rest = &line[at + 13..];
            push_unique(&mut found, rest.split('\'').next().unwrap_or(""));
        }
        // "Failure message: Mod alexscaves requires citadel 2.6.0 or above"
        if let Some(rest) = line.trim().strip_prefix("Failure message: Mod ") {
            push_unique(&mut found, rest.split_whitespace().next().unwrap_or(""));
        }
        // Mixin handlers carry the mod id: handler$fia000$xaerominimap$onClose
        for marker in [
            "handler$",
            "redirect$",
            "modify$",
            "wrapOperation$",
            "localvar$",
            "wrapMethod$",
        ] {
            if let Some(at) = line.find(marker) {
                let parts: Vec<&str> = line[at + marker.len()..].split('$').collect();
                if parts.len() >= 3 {
                    push_unique(&mut found, parts[1]);
                }
            }
        }
    }
    // Fabric API modules usually only relay another mod's error; rank them last.
    found.sort_by_key(|id| id.starts_with("fabric-") || id.starts_with("fabric_"));
    found
}

fn first_exception(text: &str) -> String {
    // The line after "Description:" (and a blank line) is the exception.
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.starts_with("Description:") {
            for next in lines.by_ref() {
                if !next.trim().is_empty() {
                    return next.trim().to_string();
                }
            }
        }
    }
    text.lines()
        .find(|l| l.contains("Exception") || l.contains("Error:"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The package root of a missing class, e.g. "com/jozufozu/flywheel/x" → "flywheel".
fn missing_package(text: &str) -> Option<String> {
    let at = text.find("NoClassDefFoundError: ")?;
    let rest = &text[at + 22..];
    // "Could not initialize class X" means X failed to start, not that it's missing.
    if rest.starts_with("Could not initialize") {
        return None;
    }
    let path = rest.split_whitespace().next()?;
    let parts: Vec<&str> = path.split(['/', '.']).collect();
    let root = if matches!(
        parts.first().copied(),
        Some("com" | "net" | "org" | "dev" | "io" | "me" | "de")
    ) {
        parts.get(2)?
    } else {
        parts.first()?
    };
    // Classes of Minecraft and Java itself mean a version mismatch instead.
    (!matches!(
        *root,
        "minecraft" | "mojang" | "lang" | "util" | "class" | "fabricmc"
    ))
    .then(|| root.to_string())
}

/// Read a crash report or log and explain it.
pub fn analyze(text: &str) -> Diagnosis {
    let raw = first_exception(text);
    let mut suspects = suspects(text);
    let mk = |cause: Cause, title: String, advice: &str, suspects: Vec<String>| Diagnosis {
        cause,
        title,
        advice: advice.to_string(),
        suspects,
        raw: raw.clone(),
    };

    if text.contains("java.lang.OutOfMemoryError") {
        return mk(
            Cause::OutOfMemory,
            "Minecraft ran out of memory".into(),
            "Give this instance more memory in its settings (Java & memory).",
            suspects,
        );
    }

    if let Some(at) = text.find("class file version ") {
        let version: u32 = text[at + 19..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0);
        if version >= 52 {
            let needs = version - 44;
            return mk(
                Cause::WrongJava { needs },
                format!("A mod needs Java {needs}"),
                "Switch this instance to the matching Java in its settings, or leave Java on automatic.",
                suspects,
            );
        }
    }
    // List.getFirst() and friends arrived in Java 21.
    if text.contains("NoSuchMethodError: 'java.lang.Object java.util.List.getFirst()'")
        || text.contains("NoSuchMethodError: 'java.lang.Object java.util.List.getLast()'")
    {
        return mk(
            Cause::WrongJava { needs: 21 },
            "A mod needs Java 21".into(),
            "Switch this instance to Java 21 in its settings, or leave Java on automatic.",
            suspects,
        );
    }

    let failures: Vec<String> = text
        .lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("Failure message: ")
                .map(str::to_string)
        })
        .filter(|l| l.contains("requires") || l.contains("only supports") || l.contains("missing"))
        .collect();
    if !failures.is_empty() {
        let mut problems = failures;
        problems.dedup();
        return mk(
            Cause::MissingDependency { problems },
            "Some mods are missing what they need".into(),
            "Install or update the mods listed, or remove the ones that need them.",
            suspects,
        );
    }
    if let Some(package) = missing_package(text) {
        push_unique(&mut suspects, &package);
        return mk(
            Cause::MissingDependency {
                problems: vec![format!(
                    "Something from \u{201c}{package}\u{201d} is missing"
                )],
            },
            format!("A mod needs {package}, which isn't installed"),
            "Install it (Health can look it up for you), or remove the mod that needs it.",
            suspects,
        );
    }

    if [
        "VK_ERROR_DEVICE_LOST",
        "GLFW error 65542",
        "GLFW error 65543",
        "Pixel format not accelerated",
        "EXCEPTION_ACCESS_VIOLATION",
    ]
    .iter()
    .any(|m| text.contains(m))
    {
        return mk(
            Cause::Graphics,
            "The graphics driver stopped responding".into(),
            "Update your graphics drivers. If you use VulkanMod or shaders, try without them.",
            suspects,
        );
    }

    if text.contains("Watchdog (Client shutdown") || text.contains("Client shutdown from post-main")
    {
        let title = match suspects.first() {
            Some(id) => format!("The game froze while closing ({id} was still busy)"),
            None => "The game froze while closing".into(),
        };
        return mk(
            Cause::HungWhileClosing,
            title,
            "Nothing was lost; your worlds were already saved. If it keeps happening, update the mod named here.",
            suspects,
        );
    }

    let version_mismatch = (raw.contains("NoSuchMethodError") || raw.contains("NoSuchFieldError"))
        && (raw.contains("net.minecraft") || raw.contains("class_"));
    if version_mismatch {
        return mk(
            Cause::WrongMinecraftVersion,
            match suspects.first() {
                Some(id) => format!("{id} was made for a different Minecraft version"),
                None => "A mod was made for a different Minecraft version".into(),
            },
            "Update that mod to a version for this Minecraft version, or remove it.",
            suspects,
        );
    }

    if let Some(first) = suspects.first()
        && (first.starts_with("fabric-") || first.starts_with("fabric_"))
    {
        let module = first.clone();
        return mk(
            Cause::ModFailed,
            format!("A mod crashed inside Fabric API ({module})"),
            "Fabric API only reported it. \u{201c}Find the broken mod\u{201d} can pin down which mod caused it in a few launches.",
            suspects,
        );
    }
    if !suspects.is_empty() {
        return mk(
            Cause::ModFailed,
            format!("{} crashed", suspects[0]),
            "Update that mod, or turn it off to check. \u{201c}Find the broken mod\u{201d} can confirm it.",
            suspects,
        );
    }

    mk(
        Cause::Unknown,
        "Minecraft crashed".into(),
        "The report doesn't point at one mod. \u{201c}Find the broken mod\u{201d} can narrow it down in a few launches.",
        suspects,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(description: &str, exception: &str, extra: &str) -> String {
        format!(
            "---- Minecraft Crash Report ----\n// Witty comment\n\nTime: 2026-11-08\nDescription: {description}\n\n{exception}\n\tat net.minecraft.client.Main.main(Main.java:1)\n{extra}\n"
        )
    }

    #[test]
    fn mixin_failure_names_the_mod() {
        let text = report(
            "Initializing game",
            "java.lang.RuntimeException: Mixin transformation of net.minecraft.class_761 failed",
            "Caused by: org.spongepowered.asm.mixin.throwables.MixinApplyError: Mixin [mixins.iris.compat.sodium.json:MixinCloudRenderer from mod iris] from phase [DEFAULT] in config [mixins.iris.compat.sodium.json] FAILED during APPLY",
        );
        let d = analyze(&text);
        assert_eq!(d.cause, Cause::ModFailed);
        assert_eq!(d.suspects, vec!["iris"]);
        assert_eq!(d.title, "iris crashed");
    }

    #[test]
    fn entrypoint_failure_names_the_mod() {
        let text = report(
            "Initializing game",
            "java.lang.RuntimeException: Could not execute entrypoint stage 'client' due to errors, provided by 'carpet' at 'carpet.CarpetServer::onGameStarted'!",
            "",
        );
        assert_eq!(analyze(&text).suspects, vec!["carpet"]);
    }

    #[test]
    fn forge_missing_dependencies_are_listed() {
        let text = report(
            "Mod loading error has occurred",
            "java.lang.Exception: Mod Loading has failed",
            "\tMod File: /x/alexscaves-2.0.2.jar\n\tFailure message: Mod alexscaves requires citadel 2.6.0 or above\n\tFailure message: Mod valkyrienskies only supports create below 6.0.0\n",
        );
        let d = analyze(&text);
        match &d.cause {
            Cause::MissingDependency { problems } => {
                assert_eq!(problems.len(), 2);
                assert!(problems[0].contains("citadel"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(d.suspects, vec!["alexscaves", "valkyrienskies"]);
    }

    #[test]
    fn hang_on_close_is_harmless_and_named() {
        let text = report(
            "Client shutdown from post-main",
            "java.lang.Error: Watchdog (Client shutdown from post-main)",
            "\tat knot//net.minecraft.client.multiplayer.ClientPacketListener.handler$fia000$xaerominimap$onClose(ClientPacketListener.java:12026)",
        );
        let d = analyze(&text);
        assert_eq!(d.cause, Cause::HungWhileClosing);
        assert!(d.title.contains("xaerominimap"), "{}", d.title);
    }

    #[test]
    fn wrong_java_and_memory_and_graphics() {
        let java = report(
            "Unexpected error",
            "java.lang.UnsupportedClassVersionError: x has been compiled by a more recent version of the Java Runtime (class file version 65.0)",
            "",
        );
        assert_eq!(analyze(&java).cause, Cause::WrongJava { needs: 21 });
        let first = report(
            "mouseClicked event handler",
            "java.lang.NoSuchMethodError: 'java.lang.Object java.util.List.getFirst()'",
            "",
        );
        assert_eq!(analyze(&first).cause, Cause::WrongJava { needs: 21 });
        let oom = report("Ticking", "java.lang.OutOfMemoryError: Java heap space", "");
        assert_eq!(analyze(&oom).cause, Cause::OutOfMemory);
        let gpu = report(
            "Unexpected error",
            "java.lang.RuntimeException: Cannot acquire next swap chain image: VK_ERROR_DEVICE_LOST",
            "",
        );
        assert_eq!(analyze(&gpu).cause, Cause::Graphics);
    }

    #[test]
    fn missing_class_and_version_mismatch() {
        let missing = report(
            "Rendering overlay",
            "java.lang.NoClassDefFoundError: com/jozufozu/flywheel/config/FlwConfig",
            "",
        );
        let d = analyze(&missing);
        assert!(d.title.contains("flywheel"), "{}", d.title);
        let mismatch = report(
            "Unexpected error",
            "java.lang.NoSuchMethodError: 'float net.minecraft.class_1702.method_35219()'",
            "\tat knot//net.minecraft.class_329.handler$zzb000$appleskin$render(class_329.java:1)",
        );
        let d = analyze(&mismatch);
        assert_eq!(d.cause, Cause::WrongMinecraftVersion);
        assert_eq!(d.suspects, vec!["appleskin"]);
    }

    #[test]
    fn fabric_api_is_not_blamed_first() {
        let text = report(
            "Initializing game",
            "java.lang.RuntimeException: Could not execute entrypoint stage 'client' due to errors, provided by 'fabric-networking-api-v1' at 'x'!",
            "\tat knot//net.minecraft.class_1.handler$abc000$polytone$init(class_1.java:1)",
        );
        assert_eq!(
            analyze(&text).suspects,
            vec!["polytone", "fabric-networking-api-v1"]
        );
        let only = report(
            "Initializing game",
            "java.lang.RuntimeException: Could not execute entrypoint stage 'client' due to errors, provided by 'fabric-screen-api-v1' at 'x'!",
            "",
        );
        assert!(analyze(&only).title.contains("inside Fabric API"));
        let init = report(
            "Ticking",
            "java.lang.NoClassDefFoundError: Could not initialize class com.example.Thing",
            "",
        );
        assert!(!analyze(&init).title.contains("Could"));
    }

    #[test]
    fn unknown_crash_still_gives_a_next_step() {
        let text = report(
            "Unexpected error",
            "java.lang.IllegalStateException: something",
            "",
        );
        let d = analyze(&text);
        assert_eq!(d.cause, Cause::Unknown);
        assert!(d.advice.contains("Find the broken mod"));
    }
}
