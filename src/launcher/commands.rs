use super::windows::{
    export_offset, load, modules, open_process, path_of, processes, thread, win_error,
};
use std::path::Path;

pub(super) fn run(root: &Path, args: &[String]) -> Result<String, String> {
    if args.iter().any(|argument| argument == "--help") {
        return Ok("OldDialogsLauncher.exe [--pid <PID>] [--quiet]\n--show / --hide: меню\n--status: состояние\n--reload: перечитать настройки\n--snapshot: снимок экрана\n--stop: остановить модуль".into());
    }
    let selected = args
        .iter()
        .position(|s| s == "--pid")
        .map(|i| {
            args.get(i + 1)
                .ok_or("Missing --pid value")
                .and_then(|s| s.parse::<u32>().map_err(|_| "Invalid PID"))
        })
        .transpose()?;
    let candidates = if let Some(pid) = selected {
        vec![pid]
    } else {
        processes()?
    };
    let expected = Path::new(r"C:\Program Files (x86)\RADMIR Games\RADMIR CRMP\gta_sa.exe");
    let mut games = Vec::new();
    for pid in candidates {
        let process = open_process(0x1000, pid);
        if process.0 == 0 {
            continue;
        }
        if let Ok(path) = path_of(process.0) {
            if path
                .to_string_lossy()
                .eq_ignore_ascii_case(&expected.to_string_lossy())
            {
                games.push(pid);
            }
        }
    }
    if games.len() != 1 {
        return Err(if games.is_empty() {
            "Запустите RADMIR CRMP из указанной папки игры".into()
        } else {
            "Найдено несколько окон игры. Укажите --pid <номер процесса>".into()
        });
    }
    let pid = games[0];
    let dll = root.join("OldDialogs.dll");
    let bytes = std::fs::read(&dll).map_err(|e| format!("{}: {e}", dll.display()))?;
    if bytes.len() < 64 {
        return Err("Invalid plugin file".into());
    }
    let pe = u32::from_le_bytes(bytes[60..64].try_into().unwrap()) as usize;
    if pe + 6 > bytes.len()
        || bytes[pe..pe + 4] != *b"PE\0\0"
        || u16::from_le_bytes(bytes[pe + 4..pe + 6].try_into().unwrap()) != 0x14c
    {
        return Err("Требуется 32-битная сборка OldDialogs.dll".into());
    }
    let plugins: Vec<_> = modules(pid)?
        .into_iter()
        .filter(|(name, _, _)| {
            name.eq_ignore_ascii_case("OldDialogs.dll")
                || name.eq_ignore_ascii_case("OldDialogs.asi")
                || (name.starts_with("OldDialogs.Runtime.") && name.ends_with(".dll"))
        })
        .collect();
    if plugins.len() > 1 {
        return Err("В игре загружено несколько копий OldDialogs. Перезапустите игру и используйте один способ загрузки: ASI или инжектор".into());
    }
    let existing = plugins.into_iter().next();
    let operation = if args.iter().any(|s| s == "--stop") {
        "OldDialogsStop"
    } else if args.iter().any(|s| s == "--status") {
        "OldDialogsState"
    } else if args.iter().any(|s| s == "--hide" || s == "--show") {
        "OldDialogsMenu"
    } else if args.iter().any(|s| s == "--reload") {
        "OldDialogsReloadSettings"
    } else if args.iter().any(|s| s == "--snapshot") {
        "OldDialogsSnapshot"
    } else {
        "OldDialogsStart"
    };
    let upgrade = args.iter().any(|s| s == "--upgrade");
    if upgrade && operation != "OldDialogsStart" {
        return Err("--upgrade используется только для запуска обновлённого модуля".into());
    }
    if existing.is_none() && operation != "OldDialogsStart" {
        return Err("Модуль ещё не загружен".into());
    }
    let process = open_process(0x43a, pid);
    if process.0 == 0 {
        return Err(win_error("Open game"));
    }
    let base = if let Some((_, path, base)) = existing {
        let same_image = path.eq_ignore_ascii_case(&dll.to_string_lossy())
            || std::fs::read(&path).ok().as_deref() == Some(bytes.as_slice());
        if !same_image {
            return Err("В игре уже загружена другая сборка OldDialogs. Перезапустите игру перед загрузкой новой DLL".into());
        }
        base
    } else {
        load(process.0, pid, &dll)?
    };
    let parameter = usize::from(args.iter().any(|s| s == "--show"));
    let code = thread(process.0, base + export_offset(&dll, operation)?, parameter)?;
    let report = serde_json::json!({"pid":pid,"operation":operation,"code":code,"module":format!("0x{base:08X}")});
    std::fs::write(
        root.join("status.json"),
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if code == 21 && operation == "OldDialogsStart" {
        return Ok(
            "Модуль загружен и ожидает кадр игры. Разверните RADMIR CRMP. Открытие меню: Insert"
                .into(),
        );
    }
    if code != 0 && operation != "OldDialogsState" {
        return Err(format!(
            "{operation}: код {code}. Подробности: last-error.txt"
        ));
    }
    Ok(if operation == "OldDialogsState" {
        format!("Состояние: {code}\nПроцесс: {pid}")
    } else if operation == "OldDialogsStart" {
        "Меню OldDialogs запущено. Открытие меню: Insert".into()
    } else {
        "Готово".into()
    })
}
