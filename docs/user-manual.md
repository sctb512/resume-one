# Resume One User Guide

**English** · [简体中文](./user-manual.zh-CN.md)

Resume One edits local JSON Resume files. [Download a preview release](https://github.com/sctb512/resume-one/releases), extract the complete ZIP, and open Resume One. The application opens a sample résumé in Preview. Choose **New Résumé** to start from an empty `basics` object, **Open JSON** to edit a file, or **Open Folder** to switch among its JSON files. The sample becomes an unsaved document only after you make an edit.

## Open, save, and recover

| Action | macOS | Windows |
| --- | --- | --- |
| New résumé | `⌘N` | `Ctrl+N` |
| Open JSON | `⌘O` | `Ctrl+O` |
| Open folder | `⌘⇧O` | `Ctrl+Shift+O` |
| Show file switcher | `⌘P` | `Ctrl+P` |
| Switch Preview and JSON | `⌘/` | `Ctrl+/` |
| Format JSON | `⌘F` | `Ctrl+F` |
| Search JSON | `⌘⇧F` | `Ctrl+Shift+F` |
| Save | `⌘S` | `Ctrl+S` |
| Save as | `⌘⇧S` | `Ctrl+Shift+S` |
| Undo / redo | `⌘Z` / `⌘⇧Z` | `Ctrl+Z` / `Ctrl+Shift+Z` |
| Manage themes | `⌘,` | `Ctrl+,` |

The file switcher opens from the filename in the toolbar. It lists regular JSON files in the first level of the current folder and up to 12 recent files. Use the arrow keys and Enter to open a file, or Esc to close the list. **Refresh** rescans the current folder. After a restart, the last folder is available to browse and the app starts in Preview. Opening a document or restoring a folder runs in the background after the window appears. **Cancel** or Esc discards a pending result. Esc closes an open menu or drawer first.

The app accepts UTF-8 résumé files up to 16 MiB. A file with invalid JSON or an invalid JSON Resume structure opens in the source editor for repair. Save requires valid content and checks whether the file has changed on disk. If Resume One reports a conflict, compare the current and disk versions before saving again. If the file is busy, wait for the other window or CLI process to finish. Keep the associated `*.resume-one.lock` file. A canceled save or failed write keeps your edits.

For a sample or new document, **Save** prompts for a path. Existing files use **Save As** to save to a different path. Closing a modified document offers **Save and Close**. When the workspace offers draft recovery, review the existing file, then choose **Recover Draft** or **Discard Draft**. Edits, formatting, saving, and export are paused until you decide. Closing or switching files without a decision keeps the draft. A recovered draft retains its original file revision, so the next save can still detect an external change.

## Edit on the page or in JSON

Press the view shortcut or use the system **View** menu. The right panel lists existing sections first, followed by available sections. Click an existing section to jump to it in the current view. Switching views attempts to keep the same section or entry in view. The toolbar and docked sections panel stay in place while the themes sidebar moves.

- In **Preview**, hover over or click the content area of **Basics** or an entry to open its form. The hit area includes empty space inside the entry. You can also focus the entry with Tab and press Enter. Move out of the form, click its close control, or press Esc to close it. The form fades in after a short hover delay and starts fading out immediately on exit, even if a field still has focus. The current input stays in the draft, including temporarily invalid data and visible IME composition. Forms can edit standard text, contact fields, social accounts, and lists such as highlights, courses, keywords, and project roles. A newly inserted item receives focus. Clearing a text field removes its key.
- To add a section, choose an insertion point on the page or beside the JSON and **then** select a section on the right. You can also select the section first and then choose an insertion point. Existing lists support per-item insertions. Press Esc to cancel. Use the minus controls to remove sections or entries. Undo and redo work for structural edits, and Preview also records applied theme changes. Hover previews do not add a history entry.
- In **JSON**, line numbers, syntax highlighting, soft wrapping, automatic indentation, and paired brackets and quotes help you edit. The initial display is formatted without rewriting the file. Editing and formatting preserve extension fields. Use the plus/minus controls beside the JSON or expand a section on the right for per-entry controls. Insertion, removal, and formatting can be undone individually. When multiple controls share a visual line, choose the intended action from the menu. Gutter controls pause while the source is invalid or the input method is composing. Duplicate top-level keys or more than 50,000 insertion/removal anchors also disable gutter actions. Click a diagnostic or focus it with Tab and press Enter to jump to the problem. Preview keeps the last valid content and pauses structural edits until the source is valid again. The source editor maintains its own text undo history. Finish composing text with an input method before switching views.

At narrower widths, the themes and sections panels open as drawers. Press Esc or click outside a drawer to close it. Selecting a section can close the drawer to expose the insertion point. The paper scales with the available space while preserving line breaks and pagination. The app follows the system reduced-motion preference.

## Themes and appearance

The left panel contains **Local** and **Online** themes. Search by name or identifier. Hovering previews the theme with the currently open résumé, and clicking applies it. Resume One generates thumbnails locally. The **Manage** panel shows directory and source diagnostics.

- **Local** uses one folder at a time. The default location is `Resume One/themes` below the system's local application data directory. The app installs 20 built-in themes there on first launch. **Manage** lets you choose or reset the folder. A selected folder can contain `*.theme.json` files at its root and `theme.json` inside each immediate theme subdirectory. Keep the full folder with any fonts and licenses. Right-click a theme to reveal its file or see its theme information. Distinct files with the same ID remain separate. Resetting or switching folders preserves old files. For synchronization, use a folder managed by iCloud Drive, OneDrive, or another sync provider. See the [built-in theme reference](../spec/builtin-theme-references.md).
- **Online** offers **Adapted Community Themes** and **Original JSON Resume** by default. The first catalog provides 67 declarative adaptations with licenses and notices. Rick Osborne is distributed under CC BY-NC-SA 4.0. The second catalog offers upstream renderers checked for local use. Choose a source above the list or reselect it to refresh. Resume One prioritizes the hovered theme, prefetches up to 20 entries near the viewport with up to three background requests, and can use a verified cache when offline. Right-click to download a verified package to your current local folder. Already downloaded identical files are reused. Current theme selection persists while browsing or downloading. The theme server receives theme requests while résumé rendering runs locally.
- Add a GitHub source from **Manage** with an `owner/repository`, a branch, tag, or commit, and the index path. The defaults are `main` and `themes/index.json`. An optional label may be up to 60 characters. The app accepts up to 16 distinct sources. Saving applies the changes. Removing a source keeps files that you have already downloaded. The two built-in sources use `migrated-index.json` and `originals/index.json` in the [theme repository](https://github.com/sctb512/resume-one-themes).

Original themes render the current résumé through the system WebView, which is WebKit on macOS and WebView2 on Windows. You can edit content in rendered sections on the page or edit any field in JSON. Use the sections panel for structural edits. Matching PDF exports and original thumbnails require Chrome, Edge, or Chromium. Set `RESUME_ONE_CHROMIUM` to an absolute browser executable path if needed. With no browser configured, the list uses simplified thumbnails. Upstream layout and fonts influence the result. Review the [theme repository](https://github.com/sctb512/resume-one-themes) for attribution, licenses, and package notes.

Choose **Appearance** on the right to adjust fonts and colors while keeping the document visible. The system **Themes → Appearance Settings** entry opens the same panel. Choose body and heading fonts separately. Search system and app-installed families, hover to try a family, and confirm by clicking or pressing Enter. **Install Font** imports TTF, OTF, TTC, or OTC into Resume One's local library. The picker accepts up to 16 files at a time. Choose **Follow Theme** to reset a font role. Personal font choices use the family's upright faces where available. Theme settings return when the override is cleared.

Click a color swatch for a palette, hue slider, preset colors, and screen sampler. Hover to try a color, click to apply it, or drag and release to commit. Esc cancels a drag, closes a picker, or returns from the appearance panel, depending on what is open. The exact value field is available for entering `#RRGGBB`. Restore default colors independently of fonts. Appearance settings live on this device and are separate from the résumé and theme files. For more on font limits and override priority, see [Appearance Settings](../spec/appearance-settings.md).

Changing color updates the page and app controls together. Screen sampling keeps no screenshots. When a selected face is missing a character, the renderer tries its CJK fallback. It reports missing code points or embedding restrictions before export. On another device, install the same personal fonts to reproduce the appearance. Theme fonts stay inside their packages and do not appear in the personal font list.

## Export

Use **Export** to save PDF, editable DOCX, Markdown, or plain text. The menu offers Chinese or English headings for preview and export. Use the left and right arrow keys to switch the heading language while the menu is open. Export uses the applied theme and confirmed appearance. The output location cannot be the source résumé. After a successful export, use **Open File** or **Show in Folder**.

- **PDF** uses the declarative theme layout or the original theme's print styles. The declarative renderer shares paper dimensions, layout, and pagination with Preview.
- **DOCX** contains editable text and embedded fonts on supported export paths. Word or LibreOffice can independently reflow content, substitute fonts, and round sizes. Original JSON Resume themes use Resume One's default layout for DOCX. The sample DOCX is about 18 MB because it includes the complete Noto fallback fonts. Verify DOCX pagination in the intended word processor. Native cross-platform Microsoft Word acceptance remains separate from file validation.
- **Markdown and plain text** use the résumé content and selected heading language. Styling from the theme and appearance panel is not applied.

The command-line tools are in `Resume One.app/Contents/MacOS/` on macOS and the extracted directory on Windows. Keep the package together because the executables share bundled fonts. For syntax and automation, see the [CLI manual](./cli-manual.md).

## Interface language and multilingual résumés

In **Manage → Interface Language**, choose **System**, **简体中文**, or **English**. The default follows the OS language preference, with English as the fallback. The website has its own language setting in its footer.

Keep Chinese and English résumé content in separate files, for example `alex.zh-CN.json` and `alex.en.json`. The filename suffix determines the initial heading language when present. Otherwise the app checks the title or summary, then uses the interface language. You can choose a heading language manually in the export menu. Neither heading nor interface settings translate your résumé content. The [English sample résumé](../assets/example.en.resume.json) is available for testing.

## Check for and install updates

Release bundles with a version marker check for updates in the background. Use **Check for Updates** in the **Open** menu or in the macOS app menu to check manually. Choose **Download update**, then **Restart and install** when ready. Preview builds receive previews or stable releases, and stable builds receive stable releases. The updater checks the download size and SHA-256 and verifies the macOS bundle signature.

Keep the installation's parent folder writable and the Windows folder named `Resume One`. Save your work before restarting. Keep résumé and custom theme folders outside the application directory. After a successful update, the previous installation is kept in `.resume-one-update-*/previous` next to the app. The updater attempts to restore the previous version if installation or launch fails. Its log is in the update folder.
