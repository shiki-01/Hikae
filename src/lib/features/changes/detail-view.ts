// 変更タブの右ペインの既定の表示（ファイル表示か、変更差分か）。純関数。

import type { ChangesView } from './view';

/** 右ペインの表示。preview はファイルの内容、diff は変更差分 */
export type DetailView = 'preview' | 'diff';

/** 選択が変わったときの既定。「変更のみ」では差分、「すべてのファイル」ではファイルの内容を出す */
export function defaultDetailView(listView: ChangesView): DetailView {
	return listView === 'tree' ? 'preview' : 'diff';
}
