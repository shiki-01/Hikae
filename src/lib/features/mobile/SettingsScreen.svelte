<script lang="ts">
	import { useQueryClient } from '@tanstack/svelte-query';
	import { t } from '#lib/i18n/index.js';
	import { mobileApi } from '#lib/api/mobile-index.js';
	import Button from '#lib/components/Button.svelte';
	import { reportError } from '#lib/features/notifications/store.svelte.js';
	import ScreenHeader from './ScreenHeader.svelte';
	import { beginRepoChange, nav } from './nav.svelte';
	import { mobileKeys, useMobileAccount } from './queries';

	const client = useQueryClient();
	const account = useMobileAccount();
	let busy = $state(false);

	async function logout(): Promise<void> {
		busy = true;
		try {
			await mobileApi.logout();
			// アカウントが無くなるので、ログインの画面へ切り替わる
			await client.invalidateQueries({ queryKey: mobileKeys.account });
		} catch (error) {
			reportError(error);
		} finally {
			busy = false;
		}
	}
</script>

<ScreenHeader title={t('mobile.settings.title')} />

<div class="flex flex-direction:column gap:4 p:4 flex:1 min-h:0 overflow:auto">
	<section class="flex flex-direction:column gap:2">
		<h2 class="m:0 type-heading">{t('mobile.settings.account')}</h2>
		{#if account.data}
			<div class="flex align-items:center gap:3">
				{#if account.data.avatarUrl}
					<img src={account.data.avatarUrl} alt="" class="size:40px r:full" />
				{/if}
				<span class="type-body">
					{t('mobile.settings.logged_in_as', { name: account.data.login })}
				</span>
			</div>
		{/if}
		<div>
			<Button variant="secondary" class="min-h:44px" disabled={busy} onclick={() => void logout()}
				>{t('mobile.settings.logout')}</Button
			>
		</div>
		<p class="m:0 type-small fg:fg-muted">{t('mobile.settings.logout_note')}</p>
	</section>

	<section class="flex flex-direction:column gap:2">
		<h2 class="m:0 type-heading">{t('mobile.settings.repo')}</h2>
		{#if nav.repo}
			<p class="m:0 type-body">{nav.repo.owner} / {nav.repo.name}</p>
		{/if}
		<div>
			<Button variant="secondary" class="min-h:44px" onclick={beginRepoChange}
				>{t('mobile.settings.change_repo')}</Button
			>
		</div>
	</section>
</div>
