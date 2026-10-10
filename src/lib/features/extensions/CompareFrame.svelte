<script lang="ts">
	// 比較（compare）を表示拡張（sandbox の iframe）に任せる（設計書 11 章・11.7）。
	// - データはホストが取る（テキストの行差分・docx・xlsx は差分の計算結果、画像は app-blob の中身）。拡張は描くだけ
	// - ready を受けてから、内容の揃った render を送る。行差分の表示方法と箇所は options で送り、表示は作り直さない
	// - 5 秒以内に ready にならない、または拡張が失敗を返したら、拡張の失敗（E20）を出す。view への切り替えはしない
	// - 時点・ファイルが変わるときは、呼び出し側が本コンポーネントを作り直す（`{#key}`）
	import { untrack } from 'svelte';
	import { t } from '#lib/i18n/index.js';
	import type { RendererInfo } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import { blobUrl, isWindowsPlatform, revForPoint } from '#lib/features/compare/image-source.js';
	import { previewKindOf, type DiffStatus } from '#lib/features/compare/preview-kind.js';
	import { useCompare, useCompareOffice, useFilePreview } from '#lib/features/compare/queries.js';
	import {
		buildCompareMessage,
		compareModelOf,
		diffStatusOf,
		type CompareModel,
		type Settled
	} from './compare-payload.js';
	import { useSideMedia } from './queries.js';
	import {
		RENDERER_PROTOCOL_VERSION,
		RENDERER_READY_TIMEOUT_MS,
		parseRendererEvent,
		type CompareOptions
	} from './renderer-protocol.js';
	import { extUrl } from './renderer-select.js';

	interface Props {
		projectId: string;
		path: string;
		/** 比べる元の時点 ID（「いま」は current） */
		from: string;
		/** 比べる先の時点 ID（「いま」は current） */
		to: string;
		fromLabel: string;
		toLabel: string;
		/** 比較に使う表示（compare を宣言するもの） */
		renderer: RendererInfo;
		/** 行差分の表示方法（テキストのときだけ使う） */
		mode: 'split' | 'unified';
		/** 行差分で強調する箇所の番号（テキストのときだけ使う） */
		activeBlock: number | null;
		/** 本体の状態が決まったときに知らせる（行差分の箇所の数など） */
		onstatus?: (status: DiffStatus) => void;
		/** 「ファイルを開く」の要求 */
		onopen?: () => void;
	}

	let {
		projectId,
		path,
		from,
		to,
		fromLabel,
		toLabel,
		renderer,
		mode,
		activeBlock,
		onstatus,
		onopen
	}: Props = $props();

	const centered =
		'flex flex-direction:column gap:3 align-items:center justify-content:center h:100% p:6';
	/** 読み込みの表示を出すまでの時間（ちらつき防止） */
	const SPINNER_DELAY_MS = 300;

	const kind = $derived(previewKindOf(path));
	const windows = isWindowsPlatform();

	// 種類ごとに必要なものだけを取る（使わないものは取らない）
	const fromMedia = useSideMedia(
		() => blobUrl(projectId, revForPoint(from), path, windows),
		() => kind === 'image'
	);
	const toMedia = useSideMedia(
		() => blobUrl(projectId, revForPoint(to), path, windows),
		() => kind === 'image'
	);
	const text = useCompare(
		() => projectId,
		() => path,
		() => from,
		() => to,
		() => kind === 'other'
	);
	const office = useCompareOffice(
		() => projectId,
		() => path,
		() => from,
		() => to,
		() => kind === 'docx' || kind === 'xlsx'
	);
	// docx が比べられないときだけ、ファイルの情報（種類・サイズ・更新日時）を取る
	const info = useCompare(
		() => projectId,
		() => path,
		() => from,
		() => to,
		() => kind === 'docx' && office.data?.kind === 'unavailable'
	);
	const fromPreview = useFilePreview(
		() => projectId,
		() => from,
		() => path,
		() => kind === 'xlsx'
	);
	const toPreview = useFilePreview(
		() => projectId,
		() => to,
		() => path,
		() => kind === 'xlsx'
	);

	/** クエリの結果のうち、組み立てに使う部分 */
	function settled<T>(query: {
		readonly isPending: boolean;
		readonly isError: boolean;
		readonly data: T | undefined;
	}): Settled<T> {
		return { pending: query.isPending, failed: query.isError, data: query.data };
	}

	const model = $derived<CompareModel>(
		compareModelOf({
			kind,
			path,
			fromLabel,
			toLabel,
			media: { from: settled(fromMedia), to: settled(toMedia) },
			text: settled(text),
			office: settled(office),
			info: settled(info),
			from: settled(fromPreview),
			to: settled(toPreview)
		})
	);
	/** 表示の対象が揃ったか（揃うまで iframe へは送らない） */
	const ready = $derived(model.state === 'ready' ? model : null);
	const options = $derived<CompareOptions>({ viewMode: mode, activeBlock });

	// iframe の状態。ready を受けてから内容を送る
	let frame = $state<HTMLIFrameElement | null>(null);
	let frameReady = $state(false);
	/** 拡張の失敗（ready が来ない・error を受けた） */
	let failed = $state(false);
	/** 送った表示の番号（失敗の報告と突き合わせる、options の送信先）。描画の再実行を起こさないよう、状態にしない */
	let activeId = 0;
	/** 番号の採番（同上） */
	let sequence = 0;
	/** 送った内容（同じ内容を二度送らない） */
	let posted: unknown = null;
	/** 送った表示の選択（render に含めた分・options で送った分。同じ選択を二度送らない） */
	let sentOptions: CompareOptions | null = null;

	const shown = $derived(!failed && frameReady && ready !== null);
	const loading = $derived(!failed && !shown);

	// 親へ本体の状態を知らせる。拡張が失敗したときは、行差分の操作を出さない
	$effect(() => {
		const next = diffStatusOf(model);
		untrack(() => onstatus?.(failed ? { textual: false, ready: true, total: 0 } : next));
	});

	// 300ms 以上かかるときだけスピナーを出す（ちらつき防止）
	let spinnerVisible = $state(false);
	$effect(() => {
		if (!loading) {
			spinnerVisible = false;
			return;
		}
		const timer = setTimeout(() => (spinnerVisible = true), SPINNER_DELAY_MS);
		return () => clearTimeout(timer);
	});

	// iframe を作ってから ready が来るまでを数える（超えたら拡張の失敗として E20 にする）
	$effect(() => {
		if (frameReady || failed) return;
		const timer = setTimeout(() => (failed = true), RENDERER_READY_TIMEOUT_MS);
		return () => clearTimeout(timer);
	});

	// 拡張からの通知。送信元は、この iframe の窓だけを受け付ける（opaque origin のため origin では確かめられない）
	$effect(() => {
		const onMessage = (event: MessageEvent) => {
			if (!frame || event.source !== frame.contentWindow) return;
			const parsed = parseRendererEvent(event.data);
			if (!parsed) return;
			switch (parsed.type) {
				case 'ready':
					frameReady = true;
					break;
				case 'error':
					if (parsed.id === activeId) failed = true;
					break;
				case 'open':
					onopen?.();
					break;
			}
		};
		window.addEventListener('message', onMessage);
		return () => window.removeEventListener('message', onMessage);
	});

	// ready の後に表示の対象が揃ったら送る（同じ内容は一度だけ）
	$effect(() => {
		const target = frame;
		const current = ready;
		if (!target || !frameReady || failed || current === null) return;
		if (current === posted) return;
		const win = target.contentWindow;
		if (!win) return;
		posted = current;
		sequence += 1;
		activeId = sequence;
		const selected = untrack(() => options);
		sentOptions = selected;
		const { message, transfer } = buildCompareMessage(sequence, current, selected);
		win.postMessage(message, '*', transfer);
	});

	// 行差分の表示方法・箇所の変更は、同じ表示へ options として送る（表示は作り直さない）。
	// 送った選択と同じなら送らない（render に含めて送ったあとに、同じ選択を重ねて送らないため）
	$effect(() => {
		const next = options;
		const target = frame;
		if (!target || !frameReady || failed || activeId === 0) return;
		if (next === sentOptions) return;
		sentOptions = next;
		target.contentWindow?.postMessage(
			{ v: RENDERER_PROTOCOL_VERSION, type: 'options', id: activeId, options: next },
			'*'
		);
	});
</script>

{#if model.state === 'failed'}
	<div class={centered}>
		<p class="m:0 type-body fg:fg-muted text-align:center">{t('compare.office.failed')}</p>
	</div>
{:else if failed}
	<!-- 拡張で描けない（拡張の失敗）。並べて比べる表示には切り替えない（同じ拡張のため） -->
	<div class={centered}>
		<p class="m:0 type-body text-align:center">{t('preview.extension.failed')}</p>
		{#if onopen}
			<Button variant="secondary" onclick={onopen}>{t('preview.extension.open')}</Button>
		{/if}
	</div>
{:else}
	<div class="flex flex-direction:column flex:1 min-h:0 min-w:0 bg:bg" data-testid="file-compare">
		<!-- sandbox は allow-scripts だけ（同一オリジンを与えない・ポップアップや遷移をさせない） -->
		<iframe
			bind:this={frame}
			src={extUrl(renderer.entryUrlPath, windows)}
			title={t('compare.title', { name: path })}
			sandbox="allow-scripts"
			referrerpolicy="no-referrer"
			loading="eager"
			class="flex:1 w:100% h:100% min-h:0 b:0 bg:bg"
			style:display={shown ? undefined : 'none'}
		></iframe>
		{#if loading && spinnerVisible}
			<div class="flex flex-direction:column flex:1 align-items:center justify-content:center">
				<Spinner />
			</div>
		{/if}
	</div>
{/if}
