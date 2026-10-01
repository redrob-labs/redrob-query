// The only place this app is allowed to name a glyph.
//
// 45-icons.md: the design system ships 252 glyphs on the brand's own geometry and forbids Feather,
// Lucide, Material and framework icon sets. It also allows exactly two sizes -- 16px beside `body`
// or `label`, 24px beside `body-lg` -- because the 24x24 box carries a 2px margin so a 16px glyph
// optically matches 20px type. This sheet had eleven sizes between 8 and 27, which is how a set drifts;
// the wrapper takes the token, not the pixel.
//
// Colour is never passed: every glyph inherits `currentColor` from the control it sits on.
import type { SVGProps } from 'react';
import { icons, type IconName } from '@redrob-labs/ui';

/** The two sanctioned sizes. 16 next to body or label text, 24 next to body-lg. */
export type IconSize = 16 | 24;

export interface IconProps extends Omit<SVGProps<SVGSVGElement>, 'color'> {
  name: IconName;
  size?: IconSize;
}

export function Icon({ name, size = 16, ...rest }: IconProps) {
  const Glyph = icons[name];
  return <Glyph width={size} height={size} {...rest} />;
}

export default Icon;
