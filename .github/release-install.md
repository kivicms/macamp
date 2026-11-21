## Установка

1. Скачайте `MacAmp-{{VERSION}}-universal.dmg` — сборка работает и на Apple
   Silicon, и на Intel.
2. Откройте DMG и перетащите **MacAmp** в **Applications**.
3. Запустите MacAmp из Applications.

Нужна macOS 12 или новее.

Сборка подписана ad-hoc и не нотаризована Apple. Если macOS пишет «MacAmp
повреждено» или «не может быть открыто», снимите карантин в Терминале:

```sh
xattr -dr com.apple.quarantine /Applications/MacAmp.app
open /Applications/MacAmp.app
```

В DMG также лежит `Install MacAmp.command` — он копирует приложение в
Applications, снимает карантин и запускает MacAmp. Если macOS не даёт открыть
скрипт, используйте правый клик → «Открыть».

## Проверка контрольной суммы

```sh
shasum -a 256 -c MacAmp-{{VERSION}}-universal.dmg.sha256
```
