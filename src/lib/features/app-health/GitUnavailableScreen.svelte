<script lang="ts">
	import { PackageX, ShieldCheck } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import type { AppHealth } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';

	interface Props {
		health: AppHealth;
		/** 確認し直している間は true */
		checking?: boolean;
		onretry: () => void;
	}

	let { health, checking = false, onretry }: Props = $props();

	const title = $derived(
		health.partsProblem === 'broken'
			? t('app_health.title.broken')
			: t('app_health.title.not_found')
	);
	const source = $derived(
		health.partsSource === 'bundled'
			? t('app_health.source_bundled')
			: health.partsSource === 'path'
				? t('app_health.source_path')
				: t('app_health.source_explicit')
	);
</script>

<main
	class="h:100vh flex flex-direction:column align-items:center justify-content:center gap:4 p:8 bg:bg fg:fg text-align:center"
>
	<PackageX size={56} class="fg:state-danger" aria-hidden="true" />
	<h1 class="m:0 type-title" role="alert">{title}</h1>
	<p class="m:0 type-body">{t('app_health.next')}</p>
	<div class="flex align-items:start gap:2 p:3 r:md bg:bg-subtle max-w:480px text-align:left">
		<ShieldCheck size={18} class="fg:state-saved flex-shrink:0 mt:2px" aria-hidden="true" />
		<p class="m:0 type-body">{t('app_health.safe')}</p>
	</div>
	<Button variant="secondary" loading={checking} onclick={onretry}>
		{t('app_health.retry')}
	</Button>
	<details class="type-small fg:fg-muted">
		<summary class="cursor:pointer">{t('app_health.detail')}</summary>
		<p class="m:0 mt:2">{t('app_health.source', { source })}</p>
	</details>
</main>
