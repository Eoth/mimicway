import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import ServiceList from '../lib/components/ServiceList.svelte';
import { resetGroupExpansionState } from '../lib/group-expansion-state.svelte.js';

const mockServices = [
  {
    name: 'svc-users',
    listen_path: '/users/*',
    real_target_url: 'http://users:80',
    is_mocked: true,
    group_name: null,
    rules: [{ name: 'r1' }],
  },
  {
    name: 'svc-orders',
    listen_path: '/orders/*',
    real_target_url: 'http://orders:80',
    is_mocked: false,
    group_name: null,
    rules: [],
  },
  {
    name: 'insee-api',
    listen_path: '/v4/api/insee/*',
    real_target_url: 'http://insee:80',
    is_mocked: true,
    group_name: 'team-a',
    rules: [{ name: 'siret' }],
  },
];

const searchPlaceholder = 'Search by name, path, URL or group...';

describe('ServiceList', () => {
  beforeEach(() => {
    // L'etat deplie/replie vit desormais dans un store partage hors du cycle
    // de vie du composant (cf group-expansion-state.svelte.js) -- isolation
    // necessaire pour que chaque test reparte d'un etat "tout replie".
    resetGroupExpansionState();
  });

  it('affiche un etat vide quand pas de services', () => {
    const { getByText } = render(ServiceList, { props: { services: [] } });
    expect(getByText('No service configured')).toBeInTheDocument();
  });

  it('affiche les groupes plies par defaut', () => {
    const { getByText, queryByText } = render(ServiceList, { props: { services: mockServices } });
    expect(getByText('team-a')).toBeInTheDocument();
    expect(getByText('No group')).toBeInTheDocument();
    expect(queryByText('svc-users')).not.toBeInTheDocument();
  });

  it('affiche les noms de groupes', () => {
    const { getByText } = render(ServiceList, { props: { services: mockServices } });
    expect(getByText('team-a')).toBeInTheDocument();
    expect(getByText('No group')).toBeInTheDocument();
  });

  it('affiche la barre de recherche quand il y a des services', () => {
    const { getByPlaceholderText } = render(ServiceList, { props: { services: mockServices } });
    expect(getByPlaceholderText(searchPlaceholder)).toBeInTheDocument();
  });

  it('filtre par nom de service', async () => {
    const { getByPlaceholderText, queryByText } = render(ServiceList, { props: { services: mockServices } });
    const search = getByPlaceholderText(searchPlaceholder);
    await fireEvent.input(search, { target: { value: 'insee' } });
    expect(queryByText('insee-api')).toBeInTheDocument();
    expect(queryByText('svc-users')).not.toBeInTheDocument();
    expect(queryByText('svc-orders')).not.toBeInTheDocument();
  });

  it('filtre par chemin d ecoute', async () => {
    const { getByPlaceholderText, queryByText } = render(ServiceList, { props: { services: mockServices } });
    const search = getByPlaceholderText(searchPlaceholder);
    await fireEvent.input(search, { target: { value: '/orders' } });
    expect(queryByText('svc-orders')).toBeInTheDocument();
    expect(queryByText('svc-users')).not.toBeInTheDocument();
  });

  it('affiche un message quand la recherche ne matche rien', async () => {
    const { getByPlaceholderText, getByText } = render(ServiceList, { props: { services: mockServices } });
    const search = getByPlaceholderText(searchPlaceholder);
    await fireEvent.input(search, { target: { value: 'zzzzz' } });
    expect(getByText('No service matches “zzzzz”')).toBeInTheDocument();
  });

  it('affiche le compteur de resultats pendant la recherche', async () => {
    const { getByPlaceholderText, getByText } = render(ServiceList, { props: { services: mockServices } });
    const search = getByPlaceholderText(searchPlaceholder);
    await fireEvent.input(search, { target: { value: 'svc' } });
    expect(getByText('2 / 3 services')).toBeInTheDocument();
  });

  it('filtre par nom de groupe', async () => {
    const { getByPlaceholderText, queryByText } = render(ServiceList, { props: { services: mockServices } });
    const search = getByPlaceholderText(searchPlaceholder);
    await fireEvent.input(search, { target: { value: 'team-a' } });
    expect(queryByText('insee-api')).toBeInTheDocument();
    expect(queryByText('svc-users')).not.toBeInTheDocument();
  });

  it('un groupe deplie manuellement reste deplie apres un demontage/remontage du composant', async () => {
    const first = render(ServiceList, { props: { services: mockServices } });
    await fireEvent.click(first.getByText('team-a'));
    expect(first.queryByText('insee-api')).toBeInTheDocument();
    first.unmount();

    // Simule le retour a la vue liste apres navigation vers l'edition d'un
    // service : ServiceList est recree de zero (nouveau montage), seul le
    // store partage (hors composant) porte l'etat deplie.
    const second = render(ServiceList, { props: { services: mockServices } });
    expect(second.queryByText('insee-api')).toBeInTheDocument();
  });

  it('selectionner un service force le deploiement de son groupe (visible au retour)', async () => {
    const onSelect = vi.fn();
    const first = render(ServiceList, { props: { services: mockServices, onSelect } });
    await fireEvent.click(first.getByText('team-a'));
    const configureBtn = first.getByRole('button', { name: 'Configure the service insee-api' });
    await fireEvent.click(configureBtn);
    expect(onSelect).toHaveBeenCalledWith('insee-api', 'team-a');
    first.unmount();

    const second = render(ServiceList, { props: { services: mockServices } });
    expect(second.queryByText('insee-api')).toBeInTheDocument();
  });

  it('un groupe replie manuellement reste replie apres un demontage/remontage du composant', async () => {
    const first = render(ServiceList, { props: { services: mockServices } });
    await fireEvent.click(first.getByText('team-a'));
    await fireEvent.click(first.getByText('team-a'));
    expect(first.queryByText('insee-api')).not.toBeInTheDocument();
    first.unmount();

    const second = render(ServiceList, { props: { services: mockServices } });
    expect(second.queryByText('insee-api')).not.toBeInTheDocument();
  });
});
