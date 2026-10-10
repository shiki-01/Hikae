<script lang="ts">
	// 変更差分（比較）の本体の振り分け。描き方は表示拡張（compare）か、過去と今を並べる（SideBySide）かのどちらか。
	// 差分の計算はホスト（Rust）、描画は拡張（設計書 11.2・11.7）。
	import { untrack } from 'svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import CompareFrame from '#lib/features/extensions/CompareFrame.svelte';
	import { useRenderers } from '#lib/features/extensions/queries.js';
	import { compareRendererFor, extOf } from '#lib/features/extensions/renderer-select.js';
	import SideBySide from './SideBySide.svelte';
	import { compareRouteOf, previewKindOf, type DiffStatus } from './preview-kind';
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
	const renderers = useRenderers();
	const renderer = $derived(
		renderers.data ? compareRendererFor(renderers.data, extOf(path)) : null
	);
	// テキストの行差分は、情報のみ（並べる）かの判定に使う。拡張で描くときは CompareFrame と同じ問い合わせを使う
	const text = useCompare(
		() => projectId,
		() => path,
		() => from,
		() => to,
		() => kind === 'other' && renderer !== null
	);
	const route = $derived(
		compareRouteOf({
			kind,
			renderers: renderers.isPending ? 'pending' : renderers.isError ? 'failed' : 'ready',
			hasCompareRenderer: renderer !== null,
			text: { pending: text.isPending, diff: text.data }
		})
	);

	// 並べて比べるときと判定の途中は、行差分の操作を出さない（拡張で描くときは CompareFrame が知らせる）
	$effect(() => {
		if (route === 'frame') return;
		const status: DiffStatus = { textual: false, ready: route === 'side', total: 0 };
		untrack(() => onstatus?.(status));
	});
</script>

{#if route === 'pending'}
	<div class="flex align-items:center justify-content:center flex:1 min-h:0">
		<Spinner />
	</div>
{:else if route === 'side'}
	<!-- 時点やファイルが変わったら作り直す（表示の失敗の状態を持つため） -->
	{#key `${path}|${from}|${to}`}
		<SideBySide {projectId} {path} {from} {to} {fromLabel} {toLabel} {onopen} />
	{/key}
{:else if renderer}
	<!-- 時点やファイルが変わったら作り直す（iframe の表示の番号と状態を持つため） -->
	{#key `${path}|${from}|${to}`}
		<CompareFrame
			{projectId}
			{path}
			{from}
			{to}
			{fromLabel}
			{toLabel}
			{renderer}
			{mode}
			{activeBlock}
			{onstatus}
			{onopen}
		/>
	{/key}
{/if}
