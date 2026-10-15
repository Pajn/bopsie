# Bopsie icon

`bopsie-icon.png` is the 1024 × 1024 opaque launcher icon used by Expo on iOS and Android.
The lavender lowercase “b” combines a music note with a record shape on a deep purple field,
matching the app's purple theme. The image has square corners so each platform can apply
its launcher mask.

Created with OpenAI image generation and resized to the required launcher dimensions.
After changing the icon, run `pnpm prebuild` and rebuild the native app.

Android shows it as an adaptive icon, whose layers the launcher masks to its own shape: the deep
purple of the field (`#3E1C78`) as the background, and `android-icon-foreground.png`, the
lavender “b” alone on transparency, at 80 % so it sits within the mask's safe zone.
`android-icon-monochrome.png` is the same shape in white, which Android tints for themed icons.
Both were cut from `bopsie-icon.png` by its green channel, which separates the glyph from the
field, so a change to the icon means cutting them again.
