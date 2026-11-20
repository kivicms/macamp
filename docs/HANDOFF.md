# MacAmp — handoff для следующей сессии

Дата среза: **2026-11-20**. Проект: `/Users/kivicms/GolandProjects/macamp`.
Пользователь говорит по-русски; хочет красивое, характерное приложение в духе
Winamp, а не минималистичный плеер. «Хотелки» приветствуются.

## Состояние одной строкой

Рабочий плеер на Rust/egui с классическим интерфейсом, реальным аудио, gapless-
очередью, эквалайзером, плейлистом, скинами, session restore, Now Playing,
windowshade, double size и стыковкой окон, плюс подписанный ad-hoc `.app`. Это
уже не спайк. `fmt`/`clippy -D warnings` чистые, 14 тестов зелёные.

## Проверки перед продолжением

```sh
cd /Users/kivicms/GolandProjects/macamp
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build
./scripts/bundle_macos.sh
```

Ожидаемо: 14 тестов, `dist/MacAmp.app`, release-бинарник ~16 МБ.
Единственный внешний warning: `block 0.1.6` (future-incompat) из souvlaki;
на текущем Rust 1.100 собирается. Наш код warning-free.

## Запуск и данные

```sh
cargo run
cargo run -- "/path/to/album" "/path/to/song.flac"
open dist/MacAmp.app
```

- Поддерживаются MP3, FLAC, WAV, M4A/AAC, OGG Vorbis. Opus не подключён.
- Данные: `~/Library/Application Support/com.macamp.MacAmp/{config.json,logs/}`.
- Для тестов каталог переопределяется `MACAMP_DATA_DIR=/tmp/...`.
- Аудио через `rodio`+Symphonia. На этой машине дефолтный Bluetooth-выход
  открылся через fallback на 44.1 кГц / 2ch / f32.
- GUI-запуск из инструментов агента иногда требует `dangerouslyDisableSandbox`,
  иначе CoreAudio может не открыться. Пользовательский запуск обычный.

## Git

`git init -b main` сделан, **коммитов нет**. Все файлы untracked:
`.github/`, `.gitignore`, `Cargo.*`, `README.md`, `assets/`, `docs/`, `scripts/`,
`src/`. Коммит делать только если пользователь попросит.

## Карта кода

| Файл | Что внутри |
|---|---|
| `src/main.rs` | eframe bootstrap, шрифты, иконка окна, размеры viewport, env-флаги |
| `src/app.rs` | всё состояние, состояние сессии, очередь/порядок, окна Main/Playlist/EQ, геометрия окон и snapping, ввод, меню, отрисовка |
| `src/audio.rs` | один `OutputStream`, Rodio `Player`, gapless-lookahead, seek/volume/balance, FFT-tap, EQ DSP |
| `src/metadata.rs` | `Track`, lofty, фоновый загрузчик тегов/папок |
| `src/spectrum.rs` | realfft-анализатор, 30 полос |
| `src/theme.rs` | Graphite/Midnight/Brass, метрики, bevel/inset/gradient |
| `src/widgets.rs` | кастомные кнопки, слайдеры, тайтлбары, LED-текст, спектр |
| `src/config.rs` | serde-конфиг, RepeatMode, пути, логи, App Support |
| `src/platform.rs` | souvlaki: Now Playing + media keys |
| `scripts/bundle_macos.sh` | release `.app`, Info.plist, icns, ad-hoc codesign |
| `.github/workflows/ci.yml` | fmt, clippy `-D warnings`, tests, build |

Старый `src/playback.rs` удалён. Не возвращать его.

## Важные технические инварианты

- eframe **0.37**: у `App` не `update`, а `logic(&mut self, ctx, frame)` для
  состояния/ввода и `ui(&mut self, ui, frame)` для отрисовки. Не откатывать на
  старый API.
- egui 0.37: для ID использовать `Id::unique(...)`; `Id::new` deprecated.
- Окна без нативной декорации: `with_decorations(false)`, кастомный тайтлбар,
  drag через `ViewportCommand::StartDrag`. Размеры:
  - Main **550×232**, windowshade **550×30**
  - Playlist **550×464** по умолчанию, user-resizable (330×200 … 1400×1200)
  - EQ **550×232**
- Main — root viewport. Playlist и EQ — **immediate viewports** через
  `show_viewport_immediate` рядом друг с другом. Их нельзя нормально снять
  встроенным screenshot-командой; для скриншотов есть dev-режимы ниже.
- **Windowshade**: двойной клик по тайтлбару или кнопка; `toggle_shade`
  отправляет Min/Max/InnerSize. В shade-режиме рисуется отдельная полоса
  `draw_shaded` с LED-временем, транспортом и EQ/PL; `draw_main` туда ветвится.
- **Double size**: `ctx.set_zoom_factor(2.0)`. Нельзя отправлять `InnerSize`
  в том же кадре, где меняется zoom: команда обрабатывается со старым
  pixels-per-point. Поэтому `pending_zoom_resize` ждёт кадр, в котором
  `ctx.zoom_factor()` уже равен цели, и только тогда шлёт размеры.
  `zoom_guard_until` (~900 мс) подавляет трекинг позиций/снаппинг на время
  асинхронного ресайза. Позиции окон хранятся в egui points при текущем zoom;
  при старте в double size они восстанавливаются после применения zoom
  (`restore_positions_after_zoom`).
- **Геометрия окон**: `rects`/`prev_rects` заполняются в draw_* из
  `viewport().outer_rect`, затем `process_window_geometry` в конце `ui()`.
  Если главное окно сдвинулось, пристёгнутые (край к краю, с общим span)
  дочерние окна получают `OuterPosition` на ту же дельту. Окно, которое
  перестало двигаться, притягивается к соседу в пределах 12 pt. При закрытии
  окна слот становится `None`. В dev-режимах `*_ROOT` геометрия отключена.
- **Ресайз плейлиста**: кастомный grip 14×14 в правом нижнем углу, тянет
  `InnerSize`; `playlist_size` сохраняется в конфиге. Синхронизация размера
  из `inner_rect` подавляется во время drag и ещё 350 мс после него.
- Шрифт Silkscreen вшит (`assets/fonts/`), у него нет кириллицы. Кириллица
  падает в системный fallback; это осознанно. Пиксельный шрифт с кириллицей —
  будущая задача.
- Skin palette в `theme.rs`; все окна читают `self.palette`, при смене скина
  просто меняется палитра.
- Размеры/шрифты в `app.rs` заданы в logical points; не умножать повторно на
  scale.

## Аудио и очередь (самое хрупкое)

- Один `MixerDeviceSink` живёт всё приложение; `Player` — очередь Rodio.
- При старте трека `player.clear()` → append текущего → `play()`.
- За 2 секунды до конца текущего трека `ensure_lookahead()` открывает и
  добавляет следующий источник в тот же `Player`. Это даёт gapless-переход.
- `TrackSource` помечает shared `(order_pos, serial)` на первом сэмпле. App
  замечает смену serial, обновляет current/play_pos.
- `appended: Option<(order_pos, serial)>` — единственный lookahead.
- При ручном Next, если lookahead совпадает, вызывается `skip_one()`, иначе
  старт нового порядка с нуля.
- Repeat One/All обрабатываются `auto_next_pos`; ручной Next идёт к следующему
  даже при Repeat One.
- Shuffle: при старте выбранного трека строится порядок «этот трек первым,
  остальные shuffle». При добавлении треков при включённом shuffle новые
  индексы дописываются в конец текущего порядка.
- Смена shuffle/repeat при уже добавленном lookahead перезапускает текущий
  трек с текущей позиции (`refresh_order_playback`), чтобы старый lookahead
  не сыграл один раз по устаревшему порядку. При паузе lookahead сбрасывается,
  при возврате play он снова ставится по новому порядку.
- Удаление из плейлиста запрещено, пока `started` (включая паузу). Это
  осознанное упрощение; при реализации удаления во время игры нужно
  перестраивать очередь/позиции и lookahead.
- Session restore ставит current/position, но **не открывает аудио**: приложение
  стартует на паузе. Play стартует с сохранённой секунды (start + seek).
- EQ обновляет коэффициенты в источнике раз в 512 сэмплов по generation-атомику
  (~11 мс). UI меняется сразу.
- EQ по умолчанию: пресет Full Bass & Treble выбран, но `eq_enabled=false`.
  ON-кнопка или выбор пресета/перетаскивание полосы включает обработку.
- DSP: 10 peaking biquad, Q=1.2, частоты `EQ_FREQUENCIES`; preamp пока 0.0;
  тесты в `audio.rs` проверяют +12 dB на 1 кГц и прозрачность выключенного EQ.
- Медиа-события souvlaki приходят через канал; callback вызывает
  `request_repaint()`, app polls в `logic`. Metadata/статус обновляются через
  `update_now_playing` и `set_progress`.
- Физические медиа-клавиши в среде агента не нажимались; инициализация
  MediaControls и callbacks проходят.

## Конфиг / сессия

`Config` (serde) хранит:
`tracks`, `current`, `position_ms`, `volume`, `balance`, `skin`,
`show_playlist`, `playlist_pos`, `playlist_size`, `main_pos`, `repeat`,
`shuffle`, `always_on_top`, `shaded`, `double_size`, `show_eq`, `eq_enabled`,
`eq_gains`, `eq_preset`, `eq_pos`.

Autosave раз в 3 секунды при dirty + на close/Drop. При добавлении новых
полей не забыть:
`Config::default`, `Config::normalize`, `save_now`, при необходимости тест
roundtrip. Окно позиций сохраняется из viewport `outer_rect`.

## UI / фичи, которые уже есть

- Main: кастомный тайтлбар, меню-поповер, shade/minimize/close, лого, LED-время,
  реальный спектр, marquee, seek, prev/play/pause/stop/next, EQ/PL/SHUF/REP,
  VOL/BAL, clutterbar.
- Shade: компактная полоса 550×30 — LED-время, prev/play/pause/stop/next,
  EQ/PL, те же кнопки окна.
- Menu: skins, Always on top, Windowshade, Double size, Playlist, Open
  files/folder, About, Quit. `⌘D` — double size.
- Окна стыкуются краями; главное окно перемещает пристёгнутые playlist/EQ.
- Playlist: ADD/FOLDER/REM, теги, duration, current/selected, двойной клик,
  wheel, кастомный скроллбар (скрывается при коротком списке), total duration,
  resize grip с сохранением размера. Удаление только когда воспроизведение
  не идёт.
- EQ: ON, cyclic presets (Flat, Full Bass & Treble, Dance, Classical, Vocal,
  Rock), curve, 10 vertical bands; drag enable автоматически.
- Skins: Graphite (default), Midnight, Brass.
- Иконка приложения + `.app`.
- Скриншоты в `docs/`: `macamp_main.png`, `macamp_shade.png`,
  `macamp_playlist.png`, `macamp_eq.png`; README показывает их.

## Dev-флаги и скриншоты

- `MACAMP_DATA_DIR=/tmp/x` — изолировать config/logs.
- `MACAMP_AUTOPLAY=1` — сразу играть; также включается, если переданы CLI-пути.
- `MACAMP_SCREENSHOT=/tmp/x.png` — root screenshot через 2 секунды и выход.
  Работает только для root viewport.
- `MACAMP_PLAYLIST_ROOT=1` — отрисовать Playlist как root; размер берётся из
  `config.playlist_size` (по умолчанию 550×464). Удобно снимать скриншот
  плейлиста через `MACAMP_SCREENSHOT`.
- `MACAMP_EQ_ROOT=1` — то же для EQ 550×232.
- Для скриншотов windowshade/double size положить в `MACAMP_DATA_DIR/config.json`
  `{"shaded": true}` и/или `{"double_size": true}`. Double size снимается в
  физических пикселях (1100×464 при Retina).
- `MACAMP_OPEN_MENU=1` — открыть меню на первом кадре для проверки/скриншота.
- `MACAMP_SCREENSHOT_PLAYLIST` удалён: eframe не отдаёт screenshot дочернего
  immediate viewport.

Пример:

```sh
MACAMP_DATA_DIR=/tmp/macamp-test-data \
MACAMP_SCREENSHOT=/tmp/main.png MACAMP_AUTOPLAY=1 \
timeout 20 ./target/debug/macamp /tmp/macamp-demo/*.flac
```

Тестовые FLAC из прошлой сессии лежали в `/tmp/macamp-demo`; `/tmp` может
очиститься. Скриншоты в `docs/` сохранятся. Для новых тестовых файлов можно
сгенерировать WAV синусами и `ffmpeg` с тегами.

## Известные ограничения на сейчас

- Нет медиатеки/SQLite, поиска, умных плейлистов; очередь — сессия.
- Нет `.wsz`; только три оригинальных скина.
- Нет MilkDrop/projectM, полноэкранного визуализатора.
- Нет обложек альбомов и предпросмотра Now Playing artwork.
- Нет Finder open-with/Apple Events; Info.plist без document types. Работают
  drag & drop и `⌘O`. Ассоциации — следующий шаг.
- Плейлист без фильтра/поиска/drag-reorder; удаление во время игры запрещено.
- EQ без AUTO/preamp.
- Нет локализации/accessibility, не notarized, нет DMG.
- `.app` подписан ad-hoc; `codesign --verify --deep --strict` проходит.
- Artifact-публикация концепт-страницы блокировалась auth-токеном
  (`ANTHROPIC_AUTH_TOKEN`); страница локальная `docs/macamp_2_0_concept.html`.
  Не ретраить без смены авторизации.

## Что делать дальше (приоритеты)

1. **Добить классическую оболочку**: фильтр и drag-reorder в плейлисте,
   удаление во время игры (аккуратно с lookahead), опционально shade для
   playlist/EQ. Windowshade, double size, snapping и ресайз уже есть.
2. **Медиатека**: SQLite (`rusqlite`) + `walkdir` + `notify` + `lofty`,
   фоновый скан, поиск, обложки (кэш на диск), плейлисты m3u/m3u8/pls,
   умные плейлисты. Новая вкладка/окно Library в том же стиле или modern skin.
3. **Скины**: импорт `.wsz` (`wsz 0.2`), атлас спрайтов, skin browser и
   предпросмотр; не бандлить чужие ассеты; поддержать viscolor.txt/pledit.txt.
4. **Визуализация**: полноэкранный FFT-виз, второй монитор; опционально
   projectM (системная libprojectm, LGPL, CMake) как отдельная фича.
5. **macOS-полировка**: Apple Events для открытия файлов из Finder, Dock menu,
   media-key проверка на живом железе, notarization + DMG, RU/EN,
   accessibility, album art в Now Playing.

Быстрые победы, которые можно сделать первыми:
- Фильтр-строка в плейлисте и drag-reorder.
- Альбомные обложки в библиотеке/Now Playing.
- `AUTO`/preamp EQ.
- ReplayGain/кроссфейд (кроссфейд потребует второго источника в микшере).
- Коммит текущего состояния, если пользователь попросит.

## Исторические документы

- `docs/winamp_style_mac_music_player_brief.md` — исходный продуктовый бриф.
- `docs/macamp_2_0_concept.html` — концепт/макет/план (сделан до реализации).
- `README.md` — актуальное состояние, запуск, шорткаты, скриншоты.

## Не делать без запроса

- Не коммитить (git-правило: только по просьбе).
- Не публиковать artifact, пока auth не исправлен.
- Не удалять оригинальную графику/лицензию Silkscreen (`assets/fonts/OFL.txt`).
- Не откатывать egui 0.37 / новый `App::logic`+`ui` API.
