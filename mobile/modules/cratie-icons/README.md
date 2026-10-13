# Bundled native icons

`pnpm build:icons` converts all 287 glyphs from the pinned Expo Feather font into Android vector
drawables and iOS vector PDF template images in `ios/Icons.xcassets`. Generated assets and the
`IconName` type are committed. `pnpm check:icons` rejects missing or stale assets after a dependency
change. Run generation again when upgrading the source font.

The shared 25-unit canvas includes the font's stroke overshoots without cropping. Curves and
holes remain vector paths; no runtime font, SVG parser or image download is involved. The source
icons are Feather, by Cole Bemis, under the included MIT license.

Android directly inflates a local drawable during the `name` prop update, with a bounded cache
of constant states and a separate mutable drawable per view. Tint is set on that view. Its native
child is laid out in the same React Native layout pass. iOS synchronously resolves the named
template image from the compiled CocoaPods resource bundle and places it in a UIImageView.
Neither platform waits for a JavaScript promise or an image-loader callback. Local asset lookup
and first vector rendering still require native work; this is not a claim of zero loading cost.

`src/ui/Icon.native.tsx` preserves the icon name, size, color and view-layout style interface.
Web keeps the font-backed fallback. Color tint uses React Native's processed colors, including
platform colors, rather than baking the current theme into the generated assets.

Android tests inflate every generated drawable with the main looper paused, verify immediate
assignment, and cover tint isolation, name changes and invalid names. They do not advance queued
tasks to make an icon appear.
