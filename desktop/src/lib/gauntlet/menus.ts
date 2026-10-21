// A verdict row's menu: Show evidence, Open in Studio, Copy id, Copy path.
// Rows a verdict cannot use are absent, not greyed (gauntlet.ts says which).

import type { MenuEntry } from '../ui/ui.svelte';
import { localRow } from '../actions';
import { layout } from '../menus/layout';
import type { Row } from '../menus/layout';
import { copyText } from '../menus/clipboard';
import { menuRowIds, type MenuRowId, type VerdictRow } from './gauntlet';

export interface VerdictVerbs {
  /** The kind's `studio` flag. */
  studio: boolean;
  openStudio(): void;
  /** Select the row and open its evidence in the detail pane. */
  showEvidence(): void;
}

export function verdictMenu(v: VerdictRow, verbs: VerdictVerbs): MenuEntry[] {
  const row: Record<MenuRowId, Row> = {
    'local:gauntlet.evidence': localRow('local:gauntlet.evidence', 'go', { label: 'Show evidence', run: verbs.showEvidence }),
    'local:gauntlet.studio': localRow('local:gauntlet.studio', 'open', { label: 'Open in Studio', run: verbs.openStudio }),
    'local:gauntlet.copy_id': localRow('local:gauntlet.copy_id', 'copy', { label: 'Copy id', run: () => void copyText(v.id), title: v.id }),
    'local:gauntlet.copy_path': localRow('local:gauntlet.copy_path', 'copy', { label: 'Copy path', run: () => void copyText(v.path, v.path), title: v.path }),
  };
  return layout(menuRowIds(v, verbs.studio).map((id) => row[id]), { fold: false });
}
