<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import FilePreview from './FilePreview.svelte';

	interface Props {
		projectId: string;
		path: string;
		/** 比べる元の時点（左。過去）の ID */
		from: string;
		/** 比べる先の時点（右。今）の ID */
		to: string;
		fromLabel: string;
		toLabel: string;
		onopen?: () => void;
	}

	// pdf・pptx・内容を表示できないファイルは、テキストの差分にせず、過去と今を並べる
	let { projectId, path, from, to, fromLabel, toLabel, onopen }: Props = $props();

	const columns = $derived([
		{ rev: from, label: fromLabel },
		{ rev: to, label: toLabel }
	]);
</script>

<div
	class="flex flex-flow:row|wrap gap:3 p:3 flex:1 min-h:0 overflow:auto bg:bg"
	data-testid="side-by-side"
>
	{#each columns as column, index (index)}
		<section
			class="flex flex-direction:column flex:1 min-w:280px min-h:240px min-w:0 r:md br:1px|solid|border overflow:hidden"
		>
			<h3 class="m:0 px:3 py:2 type-small font-weight:700 bb:1px|solid|border bg:bg-subtle">
				{column.label}
			</h3>
			<!-- 時点やファイルが変わったら作り直す（表示の失敗の状態を持つため） -->
			{#key `${path}|${column.rev}`}
				<FilePreview
					{projectId}
					rev={column.rev}
					{path}
					label={column.label}
					missingText={t('preview.missing_side')}
					{onopen}
				/>
			{/key}
		</section>
	{/each}
</div>
