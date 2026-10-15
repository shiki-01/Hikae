<script lang="ts">
	import { formatDateTime } from '#lib/i18n/format.js';
	import { t } from '#lib/i18n/index.js';
	import type { MobileVersion } from '#lib/api/mobile-types.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import ErrorNotice from './ErrorNotice.svelte';
	import { nav } from './nav.svelte';
	import { useFileHistory } from './queries';

	interface Props {
		owner: string;
		name: string;
		path: string;
		fileName: string;
		onclose: () => void;
		/** 過去の版を表示する */
		onview: (version: MobileVersion) => void;
		/** 過去の版を端末へ保存する */
		ondownload: (version: MobileVersion) => void;
	}

	let { owner, name, path, fileName, onclose, onview, ondownload }: Props = $props();

	// 表示しているあいだだけ取得する（閉じたら取り直さない）
	const history = useFileHistory(
		() => owner,
		() => name,
		() => path,
		() => true
	);
</script>

<Dialog open title={t('mobile.file.history_title')} description={fileName} {onclose}>
	{#if history.isPending}
		<div class="flex justify-content:center p:6"><Spinner /></div>
	{:else if history.isError}
		<ErrorNotice
			error={history.error}
			onretry={() => void history.refetch()}
			onlogin={() => (nav.needsLogin = true)}
		/>
	{:else if history.data && history.data.length === 0}
		<p class="m:0 type-body fg:fg-muted">{t('mobile.file.history_empty')}</p>
	{:else if history.data}
		<p class="m:0 mb:3 type-small fg:fg-muted">{t('mobile.file.version_note')}</p>
		<ul class="list-style:none m:0 p:0 flex flex-direction:column gap:2">
			{#each history.data as version (version.sha)}
				<li class="flex flex-direction:column gap:2 p:3 r:md b:1px|solid|border">
					<span class="type-small fg:fg-muted"
						>{t('mobile.file.version_when', {
							when: formatDateTime(new Date(version.date)),
							name: version.author
						})}</span
					>
					<span class="type-body">{version.message}</span>
					<div class="flex flex-wrap:wrap gap:2">
						<Button variant="secondary" class="min-h:44px" onclick={() => onview(version)}
							>{t('mobile.file.view_version')}</Button
						>
						<Button variant="ghost" class="min-h:44px" onclick={() => ondownload(version)}
							>{t('mobile.file.download_version')}</Button
						>
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</Dialog>
