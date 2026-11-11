# Translating MicLED

MicLED speaks German and English. You can add your language without Xcode and without programming: all texts of the app and of its error messages are in one file that you fill in with [Poedit](https://poedit.net) (free) or any text editor.

## 1. Get the file

- **New language:** download [`translations/template.pot`](translations/template.pot).
- **Update an existing language:** download `translations/<code>.po` from the [`translations`](translations) folder, e.g. `fr.po`. Texts that are new or changed since the last update are empty or marked as “fuzzy” (Poedit: “Needs work”).

## 2. Translate

With Poedit: *File › New from POT/PO file…*, choose `template.pot` and your language. For an existing `.po` file use *File › Open*.

Every text shows:

- the **English original** (translate this one),
- as context, mostly the **German original** (MicLED was written in German, so it can help with the meaning); for the permission prompts of macOS it is the name of the setting,
- a **note** where the text appears, e.g. “Settings window”, “Menu” or “Error message: Philips Hue”.

Please keep in mind:

- **Keep placeholders exactly as they are:** `{e}`, `{host}`, `{code}` …, `%@`, `%lld` and `${applicationName}`. You may move them within the sentence. To change the order of `%@`, `%lld` and the like, number all of them, e.g. `%2$lld … %1$@` for `%@ … %lld`. A text with a missing, extra or misspelled placeholder counts as not translated and shows in English.
- **Fuzzy texts:** the English original changed after the text was translated. Check the translation and remove the “fuzzy” mark (Poedit: turn off “Needs work”). Until then MicLED shows the English text.
- **Missing texts** are fine. MicLED shows them in English, so you can send a partial translation.
- **Plural forms:** texts marked “Plural form “one”” and “Plural form “other”” are the singular and plural of the same sentence.
- Use the style of macOS in your language, e.g. the words Apple uses in System Settings.
- The texts on the Pixoo 64 and the AWTRIX clock (“ON AIR”, “FOCUS”, “TIL 14:30”) stay in English, because the pixel font only has a few letters.

In a text editor, write your translation into the `msgstr ""` line below each `msgid`, and put your language code into the header line `"Language: \n"`, e.g. `"Language: fr\n"`.

## 3. Send it

Either way works:

- **Pull request:** save the file as `translations/<code>.po` (e.g. `translations/fr.po`) and open a pull request. If you can run Rust, run `scripts/translations.sh import translations/<code>.po` first; otherwise we do it.
- **Issue:** open an issue with the template [“New translation”](https://github.com/toolwerk/micled/issues/new?template=translation.yml) and attach the `.po` file (zip it if GitHub refuses the file type).

## For maintainers

```bash
scripts/translations.sh import ~/Downloads/fr.po            # language from the file header
scripts/translations.sh import ~/Downloads/x.po --lang pt-BR
scripts/translations.sh                                      # after any text change
```

`import` saves the file as `translations/<code>.po`, writes the language into the string catalogs of the app (`apps/MicLED/Resources/*.xcstrings`) and the error messages into `crates/micled-core/src/locales.json`, and lists every text that is missing, fuzzy or invalid. Those appear in English: in the catalogs as the English text with the state “needs review”, in the core because `locales.json` only holds valid translations.

Without arguments the script brings `translations/template.pot` and every `translations/<code>.po` up to date with the texts in the code: new texts are added empty, texts whose English changed become fuzzy, removed texts disappear. `cargo test` fails while this is forgotten. German and English are not part of this: they live in the catalogs and in `tr!("deutsch", "english")` in the code.
