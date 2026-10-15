//! Interface language. Strings are written in English in the code and looked up here at draw time,
//! so switching the language in Settings applies on the next frame.
use std::sync::atomic::{AtomicU8, Ordering};

static LANG: AtomicU8 = AtomicU8::new(0); // 0 English, 1 Russian, 2 Ukrainian

/// `code`: "en" | "ru" | "uk" | anything else follows the Windows display language.
pub fn set(code: &str) {
    let l = match code {
        "en" => 0,
        "ru" => 1,
        "uk" => 2,
        _ => system(),
    };
    LANG.store(l, Ordering::Relaxed);
}

fn system() -> u8 {
    // primary language id of the Windows UI language: 0x19 Russian, 0x22 Ukrainian
    match unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() } & 0x3ff {
        0x19 => 1,
        0x22 => 2,
        _ => 0,
    }
}

/// The current language's version of an English UI string (the string itself when there is none).
pub fn tr(s: &str) -> &str {
    let l = LANG.load(Ordering::Relaxed);
    if l == 0 {
        return s;
    }
    match TABLE.iter().find(|r| r.0 == s) {
        Some(r) => if l == 1 { r.1 } else { r.2 },
        None => s,
    }
}

/// `tr` for a template with one `{}`.
pub fn tr1(template: &str, arg: &str) -> String {
    tr(template).replacen("{}", arg, 1)
}

#[rustfmt::skip]
const TABLE: &[(&str, &str, &str)] = &[
    // dock and window buttons
    ("Previous", "Предыдущий", "Попередній"),
    ("Next", "Следующий", "Наступний"),
    ("Play", "Воспроизвести", "Відтворити"),
    ("Pause", "Пауза", "Пауза"),
    ("Mute", "Выключить звук", "Вимкнути звук"),
    ("Unmute", "Включить звук", "Увімкнути звук"),
    ("Slower", "Медленнее", "Повільніше"),
    ("Faster", "Быстрее", "Швидше"),
    ("Playback speed {}. Click to reset, hold for presets", "Скорость {}. Нажмите для сброса, удерживайте для выбора", "Швидкість {}. Натисніть для скидання, утримуйте для вибору"),
    ("Audio track", "Аудиодорожка", "Аудіодоріжка"),
    ("Audio", "Аудио", "Аудіо"),
    ("Subtitles", "Субтитры", "Субтитри"),
    ("Settings", "Настройки", "Налаштування"),
    ("Open", "Открыть", "Відкрити"),
    ("Fullscreen", "Полный экран", "Повний екран"),
    ("More", "Ещё", "Ще"),
    ("Keep on top", "Поверх всех окон", "Поверх усіх вікон"),
    ("Don't keep on top", "Не держать поверх окон", "Не тримати поверх вікон"),
    ("Minimize", "Свернуть", "Згорнути"),
    ("Maximize", "Развернуть", "Розгорнути"),
    ("Restore", "Восстановить", "Відновити"),
    ("Close", "Закрыть", "Закрити"),
    // menus and toasts
    ("Audio Tracks", "Аудиодорожки", "Аудіодоріжки"),
    ("Playback Speed", "Скорость воспроизведения", "Швидкість відтворення"),
    ("Off", "Выкл.", "Вимк."),
    ("On", "Вкл.", "Увімк."),
    ("Add Subtitle File…", "Добавить файл субтитров…", "Додати файл субтитрів…"),
    ("Added subtitle file", "Субтитры добавлены", "Субтитри додано"),
    ("Volume {}", "Громкость {}", "Гучність {}"),
    ("Keep on Top", "Поверх всех окон", "Поверх усіх вікон"),
    ("Normal Window Order", "Обычный порядок окон", "Звичайний порядок вікон"),
    ("Sound On", "Звук включён", "Звук увімкнено"),
    ("Muted", "Звук выключен", "Звук вимкнено"),
    ("Copied to clipboard", "Скопировано", "Скопійовано"),
    ("Can’t play “{}”", "Не удаётся воспроизвести «{}»", "Не вдається відтворити «{}»"),
    ("Can’t open “{}”", "Не удаётся открыть «{}»", "Не вдається відкрити «{}»"),
    ("Nothing playable there", "Там нечего воспроизводить", "Там нічого відтворювати"),
    ("This audio output can't take Dolby/DTS directly – decoding it instead", "Это устройство не принимает Dolby/DTS напрямую, звук декодируется", "Цей пристрій не приймає Dolby/DTS напряму, звук декодується"),
    ("Drop a video or audio file here", "Перетащите сюда видео или аудио", "Перетягніть сюди відео або аудіо"),
    ("or press O", "или нажмите O", "або натисніть O"),
    ("Media Files", "Медиафайлы", "Медіафайли"),
    // settings
    ("File Info", "О файле", "Про файл"),
    ("Language", "Язык", "Мова"),
    ("Auto", "Авто", "Авто"),
    ("Glass Style", "Стиль стекла", "Стиль скла"),
    ("Regular", "Матовое", "Матове"),
    ("Clear", "Прозрачное", "Прозоре"),
    ("Dock Shape", "Форма панели", "Форма панелі"),
    ("Single", "Единая", "Єдина"),
    ("Grouped", "Группы", "Групи"),
    ("Reduce Transparency", "Меньше прозрачности", "Менше прозорості"),
    ("Increase Contrast", "Больше контраста", "Більше контрасту"),
    ("Reduce Motion", "Меньше движения", "Менше руху"),
    ("Glass Transparency", "Прозрачность стекла", "Прозорість скла"),
    ("Background Extension", "Подсветка полей", "Підсвічування полів"),
    ("Fill letterbox bars with the video's edge light", "Заливает чёрные поля светом от краёв кадра", "Заповнює чорні поля світлом від країв кадру"),
    ("Hardware Decoding", "Аппаратное декодирование", "Апаратне декодування"),
    ("Surround Passthrough", "Передача объёмного звука", "Передавання об'ємного звуку"),
    ("Sends Dolby (incl. Atmos) and DTS (incl. DTS:X) untouched to an AV receiver or soundbar over HDMI. Only with such a device; volume is then set on the receiver.",
     "Отправляет Dolby (вкл. Atmos) и DTS (вкл. DTS:X) без изменений на ресивер или саундбар по HDMI. Только с таким устройством; громкость тогда регулируется на ресивере.",
     "Надсилає Dolby (з Atmos) і DTS (з DTS:X) без змін на ресивер або саундбар через HDMI. Лише з таким пристроєм; гучність тоді регулюється на ресивері."),
    ("Resume Where I Left Off", "Продолжать с места остановки", "Продовжувати з місця зупинки"),
    ("Loop", "Повтор", "Повтор"),
    ("Repeats the current video or track", "Повторяет текущее видео или трек", "Повторює поточне відео або трек"),
    ("Play Next File in Folder", "Следующий файл в папке", "Наступний файл у теці"),
    ("Continues with the next file when one ends", "После окончания включает следующий файл", "Після закінчення вмикає наступний файл"),
    ("Generate Artwork for Tracks Without Cover", "Генерировать обложки для треков без обложки", "Генерувати обкладинки для треків без обкладинки"),
    ("Uses the local ComfyUI studio if it's installed", "Использует локальную студию ComfyUI, если она установлена", "Використовує локальну студію ComfyUI, якщо її встановлено"),
    ("Quick Look in Explorer", "Быстрый просмотр в Проводнике", "Швидкий перегляд у Провіднику"),
    ("Hold Space on a video or audio file in Explorer or on the desktop to preview it", "Удерживайте пробел на видео или аудио в Проводнике или на рабочем столе, чтобы посмотреть его", "Утримуйте пробіл на відео чи аудіо у Провіднику або на робочому столі, щоб переглянути його"),
    ("Check for Updates", "Проверять обновления", "Перевіряти оновлення"),
    ("Looks for a new version on GitHub at startup", "Ищет новую версию на GitHub при запуске", "Шукає нову версію на GitHub під час запуску"),
    ("Accent", "Акцент", "Акцент"),
    // updates
    ("Update to {}", "Обновить до {}", "Оновити до {}"),
    ("Version {} is available", "Доступна версия {}", "Доступна версія {}"),
    ("Downloading update…", "Загрузка обновления…", "Завантаження оновлення…"),
    ("Update failed: {}", "Не удалось обновить: {}", "Не вдалося оновити: {}"),
    // file information
    ("File Information", "Информация о файле", "Інформація про файл"),
    ("Nothing is playing.", "Ничего не воспроизводится.", "Нічого не відтворюється."),
    ("File", "Файл", "Файл"),
    ("Name", "Имя", "Ім'я"),
    ("Folder", "Папка", "Тека"),
    ("Size", "Размер", "Розмір"),
    ("Container", "Контейнер", "Контейнер"),
    ("Duration", "Длительность", "Тривалість"),
    ("Chapters", "Главы", "Розділи"),
    ("Video", "Видео", "Відео"),
    ("Codec", "Кодек", "Кодек"),
    ("Resolution", "Разрешение", "Роздільність"),
    ("Frame rate", "Частота кадров", "Частота кадрів"),
    ("Bitrate", "Битрейт", "Бітрейт"),
    ("Pixel format", "Формат пикселей", "Формат пікселів"),
    ("Color", "Цвет", "Колір"),
    ("HW decoding", "Аппаратное декодирование", "Апаратне декодування"),
    ("Yes (tone-mapped to SDR)", "Да (преобразуется в SDR)", "Так (перетворюється в SDR)"),
    ("Sample rate", "Частота дискретизации", "Частота дискретизації"),
    ("Channels", "Каналы", "Канали"),
    ("Tags", "Теги", "Теги"),
    ("Tracks", "Дорожки", "Доріжки"),
    ("None in this file", "В этом файле нет", "У цьому файлі немає"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_and_fallback() {
        set("ru");
        assert_eq!(tr("Settings"), "Настройки");
        assert_eq!(tr("not in the table"), "not in the table");
        assert_eq!(tr1("Volume {}", "50%"), "Громкость 50%");
        set("uk");
        assert_eq!(tr("Folder"), "Тека");
        set("en");
        assert_eq!(tr("Settings"), "Settings");
    }

    #[test]
    fn no_duplicate_keys() {
        for (i, a) in TABLE.iter().enumerate() {
            assert!(TABLE[i + 1..].iter().all(|b| b.0 != a.0), "duplicate: {}", a.0);
        }
    }
}
