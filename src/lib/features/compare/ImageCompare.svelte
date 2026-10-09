<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import SegmentedControl from '#lib/components/SegmentedControl.svelte';
	import { diffPixels, parseHexColor, renderDiff, type PixelDiff, type Pixels } from './image-diff';
	import { blobUrl, isWindowsPlatform, revForPoint } from './image-source';

	interface Props {
		projectId: string;
		path: string;
		/** 比べる元の時点 ID（「いま」は current） */
		from: string;
		/** 比べる先の時点 ID（「いま」は current） */
		to: string;
		leftLabel: string;
		rightLabel: string;
	}

	let { projectId, path, from, to, leftLabel, rightLabel }: Props = $props();

	type Mode = 'side' | 'overlay' | 'diff';
	type Size = { width: number; height: number };
	type Load =
		{ kind: 'loading' } | { kind: 'ok'; size: Size } | { kind: 'absent' } | { kind: 'error' };

	/** 高さのうち、ラベルや操作部品が占める分（画像の縮尺を決めるときに差し引く） */
	const LABEL_HEIGHT = 28;
	const SLIDER_HEIGHT = 48;
	const GAP = 16;

	const windows = isWindowsPlatform();
	// 対象が変わるときは呼び出し側が本コンポーネントを作り直す（`{#key}`）
	const fromUrl = $derived(blobUrl(projectId, revForPoint(from), path, windows));
	const toUrl = $derived(blobUrl(projectId, revForPoint(to), path, windows));

	let mode = $state<Mode>('side');
	let fromLoad = $state<Load>({ kind: 'loading' });
	let toLoad = $state<Load>({ kind: 'loading' });
	let position = $state(50);
	let boxWidth = $state(0);
	let boxHeight = $state(0);

	// 画素を読むための非表示の画像（読み込み状態の判定もこれで行う）
	let fromDecoder = $state<HTMLImageElement>();
	let toDecoder = $state<HTMLImageElement>();

	/** 差分強調の結果。画素の配列は再描画のため別に持つ */
	type Diffed =
		| { kind: 'idle' }
		| { kind: 'ready'; diff: PixelDiff; fromSize: Size; toSize: Size }
		| { kind: 'blocked' };
	let diffed = $state<Diffed>({ kind: 'idle' });
	let diffRgba = $state.raw<Uint8ClampedArray | null>(null);
	let diffCanvas = $state<HTMLCanvasElement>();

	const fromSize = $derived(fromLoad.kind === 'ok' ? fromLoad.size : null);
	const toSize = $derived(toLoad.kind === 'ok' ? toLoad.size : null);
	const bothOk = $derived(fromSize !== null && toSize !== null);
	/** 両者の最大（重ねる・差分強調の枠。片方だけのときはそれ） */
	const union = $derived<Size | null>(
		fromSize && toSize
			? {
					width: Math.max(fromSize.width, toSize.width),
					height: Math.max(fromSize.height, toSize.height)
				}
			: (fromSize ?? toSize)
	);

	/** 枠に収めるための縮尺。大きい画像だけを縮め、小さい画像は拡大しない */
	function fit(size: Size | null, availWidth: number, availHeight: number): number {
		if (!size || availWidth <= 0 || availHeight <= 0) return 0;
		return Math.min(1, availWidth / size.width, availHeight / size.height);
	}

	const sideScale = $derived(fit(union, (boxWidth - GAP) / 2, boxHeight - LABEL_HEIGHT));
	const stageScale = $derived(
		mode === 'overlay'
			? fit(union, boxWidth, boxHeight - SLIDER_HEIGHT)
			: fit(union, boxWidth, boxHeight)
	);

	function sizeStyle(size: Size | null, scale: number): string {
		if (!size) return '';
		return `width:${size.width * scale}px;height:${size.height * scale}px`;
	}

	/** 画像の読み込み失敗を、「ファイルなし」（404）と「読めない」に分ける */
	async function probe(url: string): Promise<Load> {
		try {
			const response = await fetch(url, { cache: 'no-store' });
			const status = response.status;
			// 本文は読まない
			await response.body?.cancel();
			return status === 404 ? { kind: 'absent' } : { kind: 'error' };
		} catch {
			return { kind: 'error' };
		}
	}

	function onLoaded(side: 'from' | 'to', img: HTMLImageElement) {
		const load: Load = {
			kind: 'ok',
			size: { width: img.naturalWidth, height: img.naturalHeight }
		};
		if (side === 'from') fromLoad = load;
		else toLoad = load;
	}

	async function onFailed(side: 'from' | 'to', url: string) {
		const load = await probe(url);
		if (side === 'from') fromLoad = load;
		else toLoad = load;
	}

	/** 画像の画素を読む。汚染された画像（別オリジンで CORS が無い）は例外になる */
	function readPixels(img: HTMLImageElement): Pixels {
		const canvas = document.createElement('canvas');
		canvas.width = img.naturalWidth;
		canvas.height = img.naturalHeight;
		const context = canvas.getContext('2d');
		if (!context) throw new Error('2d context is not available');
		context.drawImage(img, 0, 0);
		const data = context.getImageData(0, 0, canvas.width, canvas.height).data;
		return { data, width: canvas.width, height: canvas.height };
	}

	// 差分強調に切り替えたとき、両方の画素を一度だけ比べる
	$effect(() => {
		if (mode !== 'diff' || !bothOk || diffed.kind !== 'idle') return;
		if (!fromDecoder || !toDecoder || !fromSize || !toSize) return;
		try {
			const a = readPixels(fromDecoder);
			const b = readPixels(toDecoder);
			const diff = diffPixels(a, b);
			diffRgba = renderDiff(diff, a, highlightColor());
			diffed = { kind: 'ready', diff, fromSize, toSize };
		} catch {
			diffed = { kind: 'blocked' };
		}
	});

	// 比べた結果の画素を、キャンバスへ描く
	$effect(() => {
		if (diffed.kind !== 'ready' || !diffRgba || !diffCanvas) return;
		const { width, height } = diffed.diff;
		diffCanvas.width = width;
		diffCanvas.height = height;
		const context = diffCanvas.getContext('2d');
		if (!context) return;
		const rgba = new Uint8ClampedArray(diffRgba);
		context.putImageData(new ImageData(rgba, width, height), 0, 0);
	});

	/** 差分の強調色（デザイントークン `--color-diff-highlight` の値）。読めなければ赤 */
	function highlightColor(): [number, number, number] {
		const value = getComputedStyle(document.documentElement)
			.getPropertyValue('--color-diff-highlight')
			.trim();
		return parseHexColor(value) ?? [255, 0, 0];
	}

	const modeOptions = $derived([
		{ value: 'side', label: t('compare.image.mode_side') },
		{ value: 'overlay', label: t('compare.image.mode_overlay') },
		{ value: 'diff', label: t('compare.image.mode_diff') }
	]);

	/** 差分強調の要約（違う画素の割合と、寸法の違い） */
	const summary = $derived.by(() => {
		if (diffed.kind !== 'ready') return null;
		const { diff, fromSize: a, toSize: b } = diffed;
		const parts: string[] = [];
		if (diff.sizeMismatch) {
			parts.push(
				t('compare.image.size_mismatch', {
					left: `${a.width}×${a.height}`,
					right: `${b.width}×${b.height}`
				})
			);
		}
		if (diff.changed === 0) {
			parts.push(t('compare.image.no_changes'));
		} else {
			parts.push(t('compare.image.changed_rate', { rate: (diff.rate * 100).toFixed(1) }));
		}
		return parts.join(' ／ ');
	});

	/** 状態の文言（読込中・なし・読めない）。画像が出せないときに使う */
	function statusText(load: Load): string | null {
		if (load.kind === 'loading') return t('compare.image.loading');
		if (load.kind === 'absent') return t('compare.image.none');
		if (load.kind === 'error') return t('compare.image.error.what');
		return null;
	}
</script>

<!-- 画素を読むための非表示の画像。crossorigin は canvas で画素を読むための前提 -->
<img
	bind:this={fromDecoder}
	src={fromUrl}
	crossorigin="anonymous"
	alt=""
	hidden
	onload={() => fromDecoder && onLoaded('from', fromDecoder)}
	onerror={() => onFailed('from', fromUrl)}
/>
<img
	bind:this={toDecoder}
	src={toUrl}
	crossorigin="anonymous"
	alt=""
	hidden
	onload={() => toDecoder && onLoaded('to', toDecoder)}
	onerror={() => onFailed('to', toUrl)}
/>

<div class="flex flex-direction:column flex:1 min-h:0 overflow:hidden">
	<div class="flex align-items:center gap:3 px:4 py:2 bb:1px|solid|border">
		<SegmentedControl bind:value={mode} ariaLabel={t('compare.mode')} options={modeOptions} />
		{#if mode === 'diff' && summary}
			<span class="type-small fg:fg-muted" aria-live="polite">{summary}</span>
		{/if}
		{#if mode === 'diff' && diffed.kind === 'blocked'}
			<span class="type-small fg:state-danger" role="alert">
				{t('compare.image.diff_unavailable')}
			</span>
		{/if}
	</div>

	<div
		class="position:relative flex:1 min-h:0 overflow:hidden flex flex-direction:column align-items:center justify-content:center"
		bind:clientWidth={boxWidth}
		bind:clientHeight={boxHeight}
	>
		{#if mode === 'side'}
			<div class="flex gap:4 w:100% h:100% align-items:flex-start justify-content:center">
				{#each [{ label: leftLabel, load: fromLoad, url: fromUrl, size: fromSize }, { label: rightLabel, load: toLoad, url: toUrl, size: toSize }] as side, index (index)}
					<figure class="m:0 flex flex-direction:column align-items:center min-w:0 flex:1">
						<figcaption class="type-small fg:fg-muted white-space:nowrap">{side.label}</figcaption>
						{#if side.load.kind === 'ok'}
							<img
								src={side.url}
								crossorigin="anonymous"
								alt={side.label}
								style={sizeStyle(side.size, sideScale)}
							/>
						{:else}
							<p class="m:0 type-small fg:fg-muted text-align:center">
								{statusText(side.load)}
							</p>
						{/if}
					</figure>
				{/each}
			</div>
		{:else if !bothOk}
			<!-- 重ねる・差分強調は両方の画像が必要 -->
			<p class="m:0 type-small fg:fg-muted text-align:center" role="status">
				{#if fromLoad.kind === 'error' || toLoad.kind === 'error'}
					{t('compare.image.error.what')}<br />
					{t('compare.image.error.data_safe')}<br />
					{t('compare.image.error.next')}
				{:else if fromLoad.kind === 'loading' || toLoad.kind === 'loading'}
					{t('compare.image.loading')}
				{:else}
					{t('compare.image.one_side_missing')}
				{/if}
			</p>
		{:else if mode === 'overlay'}
			<div class="flex flex-direction:column align-items:center gap:2 w:100% h:100%">
				<div class="position:relative flex-shrink:0" style={sizeStyle(union, stageScale)}>
					<img
						class="position:absolute left:0 top:0"
						src={toUrl}
						crossorigin="anonymous"
						alt={rightLabel}
						style={sizeStyle(toSize, stageScale)}
					/>
					<img
						class="position:absolute left:0 top:0"
						src={fromUrl}
						crossorigin="anonymous"
						alt={leftLabel}
						style="{sizeStyle(fromSize, stageScale)};clip-path:inset(0 {(union?.width ?? 0) *
							stageScale *
							(1 - position / 100)}px 0 0)"
					/>
					<div
						class="position:absolute top:0 bottom:0 w:2px bg:accent"
						style="left:{position}%"
						aria-hidden="true"
					></div>
				</div>
				<div class="flex align-items:center gap:3 w:100% px:4" style="height:{SLIDER_HEIGHT}px">
					<span class="type-small fg:fg-muted">{leftLabel}</span>
					<input
						class="flex:1"
						type="range"
						min="0"
						max="100"
						step="1"
						bind:value={position}
						aria-label={t('compare.image.slider')}
					/>
					<span class="type-small fg:fg-muted">{rightLabel}</span>
				</div>
			</div>
		{:else if diffed.kind === 'ready'}
			<canvas
				bind:this={diffCanvas}
				style={sizeStyle(union, stageScale)}
				aria-label={t('compare.image.mode_diff')}
			></canvas>
		{/if}
	</div>
</div>
