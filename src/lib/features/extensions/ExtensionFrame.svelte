<script lang="ts">
	// ファイルの表示（view）を、表示拡張（sandbox の iframe）に任せる（設計書 11 章）。
	// - 入口は `list_renderers` の結果から拡張子で決める。iframe は `ready` を待ってから、内容を postMessage で送る
	// - 5 秒以内に ready にならない、または拡張が失敗を返したときは、拡張の失敗の文言と「ファイルを開く」を出す
	// - 時点やファイルが変わるときは、呼び出し側が本コンポーネントを作り直す（`{#key}`）
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import { blobUrl, isWindowsPlatform, revForPoint } from '#lib/features/compare/image-source.js';
	import { previewKindOf } from '#lib/features/compare/preview-kind.js';
	import { useFilePreview } from '#lib/features/compare/queries.js';
	import { useMediaBytes, useRenderers } from './queries.js';
	import { parseRendererEvent, RENDERER_READY_TIMEOUT_MS } from './renderer-protocol.js';
	import {
		buildRenderMessage,
		mediaMimeOf,
		outcomeOfPreview,
		type RenderOutcome
	} from './render-content.js';
	import { extOf, extUrl, rendererFor } from './renderer-select.js';

	interface Props {
		projectId: string;
		/** 時点 ID（current・this・cloud・短縮 ID）。「いま」は current */
		rev: string;
		path: string;
		/** 時点の表示名（支援技術向けの領域の名前） */
		label: string;
		/** 無いときの文言。指定があれば、その時点にファイルが無いときはこの文言を出す（並べて比べる列） */
		missingText?: string;
		/** 既定のアプリで開く（表示できないとき・「ファイルを開く」の要求） */
		onopen?: () => void;
	}

	let { projectId, rev, path, label, missingText, onopen }: Props = $props();

	const centered =
		'flex flex-direction:column gap:3 align-items:center justify-content:center h:100% p:6';
	/** 読み込みの表示を出すまでの時間（ちらつき防止） */
	const SPINNER_DELAY_MS = 300;

	const kind = $derived(previewKindOf(path));
	const ext = $derived(extOf(path));
	const windows = isWindowsPlatform();
	const preview = useFilePreview(
		() => projectId,
		() => rev,
		() => path
	);
	const renderers = useRenderers();
	const renderer = $derived(renderers.data ? rendererFor(renderers.data, ext) : null);
	const mediaUrl = $derived(blobUrl(projectId, revForPoint(rev), path, windows));
	// 画像・pdf は、内容ではなく app-blob で中身を読む。存在の確認は内容の取得（binary）で行う
	const needsMedia = $derived(
		(kind === 'image' || kind === 'pdf') &&
			preview.data?.kind === 'unavailable' &&
			preview.data.reason === 'binary'
	);
	const media = useMediaBytes(
		() => mediaUrl,
		() => needsMedia
	);

	/** 表示拡張へ渡す内容（まだ揃わなければ null） */
	const outcome = $derived.by((): RenderOutcome | null => {
		const data = preview.data;
		if (!data) return null;
		if (needsMedia) {
			if (media.isError) return { state: 'unavailable', reason: 'failed' };
			const mime = mediaMimeOf(ext);
			if (!media.data || mime === null) return null;
			return { state: 'ok', content: { kind: 'bytes', mime, data: media.data } };
		}
		return outcomeOfPreview(data);
	});

	// iframe の状態。ready を受けてから内容を送り、表示は内容を送ったあとに出す
	let frame = $state<HTMLIFrameElement | null>(null);
	let ready = $state(false);
	let failed = $state(false);
	/** 送った表示の番号（失敗の報告と突き合わせる）。描画の再実行を起こさないよう、状態にしない */
	let activeId = 0;
	/** 送った表示の番号の採番（同上） */
	let sequence = 0;

	const missingNative = $derived(preview.data?.kind === 'not_found' && missingText !== undefined);
	/** 拡張で表示できない（拡張が無い・失敗した・読み込みが一定時間で終わらなかった） */
	const fallback = $derived(
		failed || renderers.isError || (renderers.isSuccess && renderer === null)
	);
	const shown = $derived(ready && outcome !== null);
	const loading = $derived(!fallback && !missingNative && !preview.isError && !shown);

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

	// iframe を作ってから ready が来るまでを数える（超えたら拡張の失敗として通常の表示へ切り替える）
	$effect(() => {
		if (!renderer || ready || failed) return;
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
					ready = true;
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

	// ready の後に内容が揃ったら送る。送るのは表示の対象（名前・拡張子・内容）だけ
	$effect(() => {
		const target = frame;
		const current = outcome;
		if (!target || !ready || failed || current === null) return;
		const win = target.contentWindow;
		if (!win) return;
		sequence += 1;
		activeId = sequence;
		const { message, transfer } = buildRenderMessage(sequence, path, current);
		win.postMessage(message, '*', transfer);
	});
</script>

<div class="flex flex-direction:column flex:1 min-h:0 min-w:0 bg:bg" data-testid="file-preview">
	{#if preview.isError}
		<div class={centered}>
			<p class="m:0 type-body fg:fg-muted text-align:center">{t('preview.failed')}</p>
		</div>
	{:else if missingNative}
		<div class={centered}>
			<p class="m:0 type-body fg:fg-muted text-align:center">{missingText}</p>
		</div>
	{:else if fallback}
		<div class={centered}>
			<p class="m:0 type-body text-align:center">{t('preview.extension.failed')}</p>
			{#if onopen}
				<Button variant="secondary" onclick={onopen}>{t('preview.extension.open')}</Button>
			{/if}
		</div>
	{:else}
		<div class="flex flex-direction:column flex:1 min-h:0 min-w:0">
			{#if renderer}
				<!-- sandbox は allow-scripts だけ（同一オリジンを与えない・ポップアップや遷移をさせない） -->
				<iframe
					bind:this={frame}
					src={extUrl(renderer.entryUrlPath, windows)}
					title={label}
					sandbox="allow-scripts"
					referrerpolicy="no-referrer"
					loading="eager"
					class="flex:1 w:100% h:100% min-h:0 b:0 bg:bg"
					style:display={shown ? undefined : 'none'}
				></iframe>
			{/if}
			{#if loading && spinnerVisible}
				<div class="flex flex-direction:column flex:1 align-items:center justify-content:center">
					<Spinner />
				</div>
			{/if}
		</div>
	{/if}
</div>
