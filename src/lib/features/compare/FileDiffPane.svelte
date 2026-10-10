<script lang="ts">
	import { untrack } from 'svelte';
	import { t } from '#lib/i18n/index.js';
	import DiffView from '#lib/components/DiffView.svelte';
	import { countBlocks } from '#lib/components/diff-rows.js';
	import ImageCompare from './ImageCompare.svelte';
	import OfficeCompare from './OfficeCompare.svelte';
	import SideBySide from './SideBySide.svelte';
	import XlsxDiff from './XlsxDiff.svelte';
	import { diffBodyOf, previewKindOf, type DiffStatus } from './preview-kind';
	import { useCompare } from './queries';

	interface Props {
		projectId: string;
		path: string;
		/** 比べる元の時点 ID（「いま」は current） */
		from: string;
		/** 比べる先の時点 ID（「いま」は current） */
		to: string;
		fromLabel: string;
		toLabel: string;
		/** 行差分の表示方法（テキストのときだけ使う） */
		mode?: 'split' | 'unified';
		/** 行差分で強調する箇所の番号（テキストのときだけ使う） */
		activeBlock?: number | null;
		/** 本体の状態が決まったときに知らせる（行差分の箇所の数など。親が前へ・次へなどを出すかを決める） */
		onstatus?: (status: DiffStatus) => void;
		onopen?: () => void;
	}

	let {
		projectId,
		path,
		from,
		to,
		fromLabel,
		toLabel,
		mode = 'split',
		activeBlock = null,
		onstatus,
		onopen
	}: Props = $props();

	const kind = $derived(previewKindOf(path));
	// 行差分は、その他のファイル（種類が決まっていないもの）だけで使う。結果の種類で表示を決める
	const text = useCompare(
		() => projectId,
		() => path,
		() => from,
		() => to,
		() => kind === 'other'
	);
	const body = $derived(diffBodyOf(kind, text.data));
	const blockTotal = $derived(text.data?.kind === 'text' ? countBlocks(text.data.rows) : 0);

	$effect(() => {
		// 親の関数（onstatus）の変化では再実行しない（親の更新がまた知らせを呼ぶ循環を避ける）
		const status: DiffStatus = {
			textual: body === 'text',
			ready: body !== 'pending',
			total: blockTotal
		};
		untrack(() => onstatus?.(status));
	});
</script>

{#if body === 'image'}
	<!-- 時点や対象が変わったら作り直す（画像の読み込み状態を持つため） -->
	{#key `${path}|${from}|${to}`}
		<ImageCompare {projectId} {path} {from} {to} leftLabel={fromLabel} rightLabel={toLabel} />
	{/key}
{:else if body === 'docx'}
	<OfficeCompare {projectId} {path} {from} {to} {onopen} />
{:else if body === 'xlsx'}
	{#key `${path}|${from}|${to}`}
		<XlsxDiff {projectId} {path} {from} {to} {onopen} />
	{/key}
{:else if body === 'side_by_side'}
	{#key `${path}|${from}|${to}`}
		<SideBySide {projectId} {path} {from} {to} {fromLabel} {toLabel} {onopen} />
	{/key}
{:else}
	<DiffView
		diff={text.data ?? null}
		loading={text.isPending}
		{mode}
		{activeBlock}
		leftLabel={fromLabel}
		rightLabel={toLabel}
		emptyText={text.isError ? t('compare.office.failed') : undefined}
		{onopen}
	/>
{/if}
