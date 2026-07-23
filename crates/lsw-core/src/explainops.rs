#[derive(Debug, Clone, Copy)]
pub struct Explanation {
    pub code: &'static str,
    pub summary: &'static str,
    pub hint: &'static str,
}

pub fn explain(code: &str) -> Option<Explanation> {
    let normalized = normalize(code);
    TABLE.iter().find(|e| e.code == normalized).copied()
}

fn normalize(code: &str) -> String {
    let trimmed = code.trim();
    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        trimmed.to_uppercase()
    } else {
        format!("LSW{digits}")
    }
}

const TABLE: &[Explanation] = &[
    Explanation {
        code: "LSW1005",
        summary: "no lsw.toml was found in this directory or any parent",
        hint: "run `lsw init` to scaffold a project, or cd into an existing one",
    },
    Explanation {
        code: "LSW1007",
        summary: "the environment was created by a newer LSW than this build supports",
        hint: "upgrade LSW, or recreate the environment with `lsw env create --force`",
    },
    Explanation {
        code: "LSW1303",
        summary: "the file is not a PE executable",
        hint: "pass a Windows .exe or .dll, such as one produced by `lsw build`",
    },
    Explanation {
        code: "LSW1501",
        summary: "the wine runtime was not found on PATH",
        hint: "install wine (e.g. `pacman -S wine` or `apt install wine`)",
    },
    Explanation {
        code: "LSW1505",
        summary: "a strict sandbox was requested but bubblewrap is not installed",
        hint: "install bubblewrap, or drop --sandbox",
    },
    Explanation {
        code: "LSW1506",
        summary: "a virtual display was requested but xvfb-run is not installed",
        hint: "install xvfb, or run with a real $DISPLAY",
    },
    Explanation {
        code: "LSW1507",
        summary: "the process is not running in this environment",
        hint: "list processes with `lsw ps` to get a valid pid",
    },
    Explanation {
        code: "LSW2001",
        summary: "no active environment is selected for this project",
        hint: "run `lsw use <name>` (or `lsw env create <name>` first)",
    },
    Explanation {
        code: "LSW2002",
        summary: "the named environment does not exist",
        hint: "create it with `lsw env create <name>`, or list with `lsw env list`",
    },
    Explanation {
        code: "LSW2004",
        summary: "the target is not something LSW can execute",
        hint: "pass a PE/ELF/script, or force a domain with --host or --windows",
    },
    Explanation {
        code: "LSW2005",
        summary: "the build command failed",
        hint: "re-run `lsw build --verbose` and read the compiler output above",
    },
    Explanation {
        code: "LSW2006",
        summary: "lsw.lock does not match the active environment",
        hint: "refresh the pins with `lsw build --update-lock`, or `lsw env restore`",
    },
    Explanation {
        code: "LSW2007",
        summary: "no build system was detected",
        hint: "add CMakeLists.txt/Cargo.toml/meson.build, or set [build] command in lsw.toml",
    },
    Explanation {
        code: "LSW2011",
        summary: "a required external tool was not found on PATH",
        hint: "install the tool named in the error message",
    },
    Explanation {
        code: "LSW2012",
        summary: "an invalid environment or project name was given",
        hint: "use a name without slashes, dots-only, or control characters",
    },
    Explanation {
        code: "LSW2023",
        summary: "the optional lsw daemon is not running",
        hint: "start it with `lswd`; most commands work without the daemon",
    },
    Explanation {
        code: "LSW2030",
        summary: "an invalid [sandbox] network value was set",
        hint: "use network = \"host\", \"isolated\", or \"none\"",
    },
    Explanation {
        code: "LSW2040",
        summary: "the MSI failed install/uninstall verification in a scratch environment",
        hint: "inspect the msiexec output in the error; rerun `lsw package --target msi --verify`",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explain_normalizes_and_looks_up() {
        assert_eq!(explain("LSW2004").unwrap().code, "LSW2004");
        assert_eq!(explain("2004").unwrap().code, "LSW2004");
        assert_eq!(explain("lsw2004").unwrap().code, "LSW2004");
        assert!(explain("LSW9999").is_none());
    }
}
