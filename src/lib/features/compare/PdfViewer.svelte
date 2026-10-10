<script lang="ts">
	import type { PDFDocumentProxy, RenderTask } from 'pdfjs-dist';
	import { SvelteMap, SvelteSet } from 'svelte/reactivity';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import {
		destroyPdfDocument,
		loadPdfDocument,
		PdfLoadError,
		type PdfFailure
	} from './pdf-document';
	import { PDF_MAX_PAGES, pdfPageWindow } from './preview-view';

	interface Props {
		/** app-blob の URL（読み取りのみ） */
		url: string;
		/** 領域の名前（支援技術向け） */
		label: string;
		onopen?: () => void;
	}

	let { url, label, onopen }: Props = $props();

	type Load =
		| { kind: 'loading' }
		| { kind: 'ready'; doc: PDFDocumentProxy }
		| { kind: 'failed'; reason: PdfFailure };

	/** ページの縦横比が分かるまでの仮の比率（A4 の縦） */
	const DEFAULT_ASPECT = '1 / 1.414';

	let load = $state.raw<Load>({ kind: 'loading' });
	let root = $state<HTMLDivElement>();
	let current = $state(1);
	/** ページごとの縦横比（描いたあとに分かる） */
	let aspects = $state.raw<Record<number, string>>({});
	let failedPages = $state.raw<Set<number>>(new Set());

	const doc = $derived(load.kind === 'ready' ? load.doc : null);
	const windowOf = $derived(pdfPageWindow(doc?.numPages ?? 0));
	const pageNumbers = $derived(Array.from({ length: windowOf.shown }, (_, index) => index + 1));

	// URL が変わったら読み直す。破棄・差し替えのときは、読み込み中の文書も破棄する
	$effect(() => {
		const target = url;
		let cancelled = false;
		let loaded: PDFDocumentProxy | null = null;
		load = { kind: 'loading' };
		current = 1;
		loadPdfDocument(target).then(
			(pdf) => {
				if (cancelled) {
					void destroyPdfDocument(pdf);
					return;
				}
				loaded = pdf;
				load = { kind: 'ready', doc: pdf };
			},
			(error: unknown) => {
				if (cancelled) return;
				load = { kind: 'failed', reason: error instanceof PdfLoadError ? error.reason : 'failed' };
			}
		);
		return () => {
			cancelled = true;
			if (loaded) void destroyPdfDocument(loaded);
		};
	});

	// 見えるページだけを描く（近づいたら描き、描いたページは残す）。描き途中のものは破棄で取り消す
	$effect(() => {
		const scroller = root;
		const pdf = doc;
		if (!scroller || !pdf) return;
		const rendered = new SvelteSet<number>();
		const tasks = new SvelteMap<number, RenderTask>();
		const visible = new SvelteSet<number>();

		const render = async (page: number, container: HTMLElement) => {
			try {
				const pdfPage = await pdf.getPage(page);
				if (!container.isConnected) {
					pdfPage.cleanup();
					return;
				}
				const base = pdfPage.getViewport({ scale: 1 });
				aspects = { ...aspects, [page]: `${base.width} / ${base.height}` };
				const width = container.clientWidth > 0 ? container.clientWidth : 600;
				// 高解像度の画面でも鮮明に描くため、devicePixelRatio を掛ける（表示の幅は CSS で合わせる）
				const dpr = window.devicePixelRatio || 1;
				const viewport = pdfPage.getViewport({ scale: (width / base.width) * dpr });
				const canvas = globalThis.document.createElement('canvas');
				canvas.width = Math.floor(viewport.width);
				canvas.height = Math.floor(viewport.height);
				canvas.className = 'w:100% h:auto display:block';
				container.appendChild(canvas);
				const task = pdfPage.render({ canvas, viewport });
				tasks.set(page, task);
				await task.promise;
				tasks.delete(page);
				pdfPage.cleanup();
			} catch (error) {
				tasks.delete(page);
				if (error instanceof Error && error.name === 'RenderingCancelledException') return;
				failedPages = new Set([...failedPages, page]);
			}
		};

		const observer = new IntersectionObserver(
			(entries) => {
				for (const entry of entries) {
					const element = entry.target as HTMLElement;
					const page = Number(element.dataset.page);
					if (entry.isIntersecting) {
						visible.add(page);
						if (!rendered.has(page)) {
							rendered.add(page);
							void render(page, element);
						}
					} else {
						visible.delete(page);
					}
				}
				if (visible.size > 0) current = Math.min(...visible);
			},
			{ root: scroller, rootMargin: '400px 0px' }
		);
		for (const element of scroller.querySelectorAll<HTMLElement>('[data-page]')) {
			observer.observe(element);
		}

		return () => {
			observer.disconnect();
			for (const task of tasks.values()) task.cancel();
			tasks.clear();
		};
	});
</script>

<div class="flex flex-direction:column flex:1 min-h:0 min-w:0 bg:bg-subtle" aria-label={label}>
	{#if load.kind === 'loading'}
		<div class="flex align-items:center justify-content:center gap:2 h:100% p:6">
			<Spinner />
			<p class="m:0 type-body fg:fg-muted">{t('preview.pdf.loading')}</p>
		</div>
	{:else if load.kind === 'failed'}
		<div
			class="flex flex-direction:column gap:3 align-items:center justify-content:center h:100% p:6"
		>
			<p class="m:0 type-body text-align:center">
				{t(load.reason === 'encrypted' ? 'preview.pdf.encrypted' : 'preview.pdf.failed')}
			</p>
			{#if onopen}
				<Button variant="secondary" onclick={onopen}>{t('file_action.open_default')}</Button>
			{/if}
		</div>
	{:else}
		<div
			class="flex align-items:center justify-content:space-between gap:3 px:4 py:2 bb:1px|solid|border"
		>
			<span class="type-small fg:fg-muted" aria-live="polite">
				{t('preview.pdf.page', { current, total: load.doc.numPages })}
			</span>
			{#if windowOf.capped}
				<span class="type-small fg:fg-muted"
					>{t('preview.pdf.capped', { count: PDF_MAX_PAGES })}</span
				>
			{/if}
		</div>
		<div bind:this={root} class="flex flex-direction:column gap:3 flex:1 min-h:0 overflow:auto p:3">
			{#each pageNumbers as page (page)}
				<div
					data-page={page}
					class="position:relative w:100% flex-shrink:0 bg:bg-raised br:1px|solid|border"
					style:aspect-ratio={aspects[page] ?? DEFAULT_ASPECT}
				>
					{#if failedPages.has(page)}
						<p class="m:0 p:3 type-small fg:fg-muted">{t('preview.pdf.failed')}</p>
					{/if}
				</div>
			{/each}
		</div>
	{/if}
</div>
