<script lang="ts">
	import { page } from '$app/state';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import { useSession } from '#lib/features/projects/queries.js';
	import { gotoLogin, reloginSearch } from '#lib/features/projects/relogin.js';
	import { useLogout } from './account';

	const session = useSession();

	let confirmOpen = $state(false);

	// ログアウトしたら、ログインの手順へ移り、終わったら一覧に戻る
	const logout = useLogout(() => {
		confirmOpen = false;
		void gotoLogin(reloginSearch({ pathname: '/', search: '' }));
	});

	// ログインし直す・ログインするときは、終わったらこの設定画面ではなく一覧へ戻る
	function login() {
		void gotoLogin(reloginSearch(page.url));
	}

	const user = $derived(session.data?.userLogin ?? null);
	const initial = $derived((user ?? '?').slice(0, 1).toUpperCase());
</script>

<section class="flex flex-direction:column gap:3 pb:4 bb:1px|solid|border">
	<h3 class="m:0 type-body font-weight:700">{t('account.title')}</h3>

	{#if session.isPending}
		<Skeleton class="h:48px" />
	{:else if session.data?.loggedIn}
		<div class="flex align-items:center justify-content:space-between gap:4">
			<div class="flex align-items:center gap:3 min-w:0">
				{#if session.data.avatarUrl}
					<img
						src={session.data.avatarUrl}
						alt=""
						referrerpolicy="no-referrer"
						class="size:40px r:full object-fit:cover flex-shrink:0"
					/>
				{:else}
					<span
						class="size:40px r:full flex align-items:center justify-content:center flex-shrink:0 bg:accent-subtle fg:accent type-body font-weight:700"
						aria-hidden="true"
					>
						{initial}
					</span>
				{/if}
				<div class="flex flex-direction:column gap:1 min-w:0">
					<span class="type-body font-weight:500 overflow-wrap:anywhere" data-testid="account-name">
						{user ?? t('account.title')}
					</span>
					{#if session.data.reauthRequired}
						<span class="type-small fg:state-unsaved">{t('account.reauth')}</span>
						<span class="type-small fg:fg-muted">{t('account.reauth_text')}</span>
					{/if}
				</div>
			</div>
			<div class="flex align-items:center gap:2 flex-shrink:0">
				{#if session.data.reauthRequired}
					<Button size="sm" onclick={login}>{t('account.relogin')}</Button>
				{/if}
				<Button size="sm" variant="secondary" onclick={() => (confirmOpen = true)}>
					{t('account.logout')}
				</Button>
			</div>
		</div>
	{:else}
		<div class="flex align-items:center justify-content:space-between gap:4">
			<div class="flex flex-direction:column gap:1 min-w:0">
				<span class="type-body font-weight:500">{t('account.not_logged_in')}</span>
				<span class="type-small fg:fg-muted">{t('account.not_logged_in_text')}</span>
			</div>
			<Button size="sm" class="flex-shrink:0" onclick={login}>{t('account.login')}</Button>
		</div>
	{/if}
</section>

<Dialog
	open={confirmOpen}
	variant="warning"
	title={t('account.logout_title')}
	description={t('account.logout_text')}
	busy={logout.isPending}
	onclose={() => (confirmOpen = false)}
>
	{#snippet actions()}
		<Button variant="secondary" disabled={logout.isPending} onclick={() => (confirmOpen = false)}>
			{t('settings.confirm_cancel')}
		</Button>
		<Button loading={logout.isPending} onclick={() => logout.mutate()}>
			{t('account.logout_confirm')}
		</Button>
	{/snippet}
</Dialog>
