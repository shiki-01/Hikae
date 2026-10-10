<script lang="ts">
	// ファイルの内容の表示（view）。表示の本体は表示拡張（ExtensionFrame）に任せる（設計書 11 章）。
	// 呼び出し側（ChangeDetail・SideBySide）は、この props をそのまま渡す。
	import ExtensionFrame from '#lib/features/extensions/ExtensionFrame.svelte';

	interface Props {
		projectId: string;
		/** 時点 ID（current・this・cloud・短縮 ID）。「いま」は current */
		rev: string;
		path: string;
		/** 時点の表示名（支援技術向けの領域の名前） */
		label: string;
		/** 無いときの文言。並べて比べる列では「この時点にはありません」にする */
		missingText?: string;
		/** 既定のアプリで開く（表示できないときの案内） */
		onopen?: () => void;
	}

	let { projectId, rev, path, label, missingText, onopen }: Props = $props();
</script>

<ExtensionFrame {projectId} {rev} {path} {label} {missingText} {onopen} />
