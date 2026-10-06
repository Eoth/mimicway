import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import ServiceForm from '../lib/components/ServiceForm.svelte';

const PURELY_MOCKED_LABEL = 'Purely mocked service';

async function setInput(el, value) {
  el.value = value;
  await fireEvent.input(el);
}

async function submitForm(container) {
  const form = container.querySelector('form');
  await fireEvent.submit(form);
}

describe('ServiceForm validation', () => {
  it('refuse un nom vide', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), '');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('The service name is required.');
  });

  it('refuse le nom reserve "api"', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'api');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('The name "api" is reserved by Mimicway');
  });

  it('refuse le nom reserve "index.html"', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'index.html');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('The name "index.html" is reserved by Mimicway');
  });

  it('accepte un listen_path vide (catch-all)', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, container } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'my-svc');
    await setInput(getByLabelText('Listen path (optional)'), '');
    await setInput(getByLabelText('Real target URL'), 'http://backend:8080');
    await submitForm(container);
    expect(onSave).toHaveBeenCalled();
  });

  it('accepte un listen_path "/" (catch-all)', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, container } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'my-svc');
    await setInput(getByLabelText('Listen path (optional)'), '/');
    await setInput(getByLabelText('Real target URL'), 'http://backend:8080');
    await submitForm(container);
    expect(onSave).toHaveBeenCalled();
  });

  it('accepte un listen_path "/*" (catch-all explicite)', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, container } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'my-svc');
    await setInput(getByLabelText('Listen path (optional)'), '/*');
    await setInput(getByLabelText('Real target URL'), 'http://backend:8080');
    await submitForm(container);
    expect(onSave).toHaveBeenCalled();
  });

  it('accepte un listen_path valide avec sous-chemin', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, container } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'my-svc');
    await setInput(getByLabelText('Listen path (optional)'), '/v1/users/*');
    await setInput(getByLabelText('Real target URL'), 'http://backend:8080');
    await submitForm(container);
    expect(onSave).toHaveBeenCalled();
  });

  it('refuse un nom contenant /', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'my/svc');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('A service name cannot contain a path separator (/ or \\).');
  });

  it('permet la soumission meme si nom existe (unicite geree par le backend par groupe)', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, container } = render(ServiceForm, {
      props: { onSave, existingNames: ['existing-svc'] },
    });

    await setInput(getByLabelText('Service name'), 'existing-svc');
    await setInput(getByLabelText('Real target URL'), 'http://backend:8080');
    await submitForm(container);
    expect(onSave).toHaveBeenCalled();
  });

  it('autorise le meme nom en edition', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const existingService = {
      name: 'existing-svc',
      listen_path: '/v1/*',
      real_target_url: 'http://backend:8080',
      is_mocked: true,
      rewrite_directory_urls: false,
      rules: [],
    };
    const { getByLabelText, container } = render(ServiceForm, {
      props: { service: existingService, existingNames: ['existing-svc'], isEdit: true, onSave },
    });

    await setInput(getByLabelText('Real target URL'), 'http://new-backend:9090');
    await submitForm(container);
    expect(onSave).toHaveBeenCalled();
  });

  it('desactive le champ nom en mode edition', async () => {
    const existingService = {
      name: 'existing-svc',
      listen_path: '/v1/*',
      real_target_url: 'http://backend:8080',
      is_mocked: true,
      rewrite_directory_urls: false,
      rules: [],
    };
    const { getByLabelText } = render(ServiceForm, {
      props: { service: existingService, isEdit: true },
    });

    expect(getByLabelText('Service name')).toBeDisabled();
  });

  it("laisse le champ nom editable lors d'un clonage (service pre-rempli sans isEdit)", async () => {
    const clonedService = {
      name: 'existing-svc-copy',
      listen_path: '/v1/*',
      real_target_url: 'http://backend:8080',
      is_mocked: true,
      rewrite_directory_urls: false,
      rules: [],
    };
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, container } = render(ServiceForm, {
      props: { service: clonedService, onSave },
    });

    const nameInput = getByLabelText('Service name');
    expect(nameInput).not.toBeDisabled();
    expect(nameInput.value).toBe('existing-svc-copy');

    await setInput(nameInput, 'renamed-clone');
    await setInput(getByLabelText('Real target URL'), 'http://backend:8080');
    await submitForm(container);
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ name: 'renamed-clone' }));
  });

  it('refuse un nom contenant un espace', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'my svc');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent(
      'A service name can only contain letters, digits, dashes (-) and underscores (_).',
    );
  });

  it('refuse un nom contenant un caractere special', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'svc@name!');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent(
      'A service name can only contain letters, digits, dashes (-) and underscores (_).',
    );
  });

  it('accepte un nom avec underscores et chiffres', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, container } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'svc_v2-42');
    await setInput(getByLabelText('Real target URL'), 'http://backend:8080');
    await submitForm(container);
    expect(onSave).toHaveBeenCalled();
  });
});

describe('ServiceForm service purement mocké', () => {
  it('cocher la case masque le champ cible et permet la creation sans cible', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, getByRole, queryByLabelText, container } = render(ServiceForm, { props: { onSave } });

    // Non coche par defaut : le champ cible est requis comme avant.
    expect(getByLabelText('Real target URL')).toBeInTheDocument();

    await fireEvent.click(getByRole('switch', { name: PURELY_MOCKED_LABEL }));
    expect(queryByLabelText('Real target URL')).not.toBeInTheDocument();

    await setInput(getByLabelText('Service name'), 'sans-cible');
    await submitForm(container);

    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        name: 'sans-cible',
        real_target_url: '',
        is_mocked: true,
      }),
    );
  });

  it('un service existant sans cible ouvre le formulaire avec la case deja cochee', () => {
    const existingService = {
      name: 'deja-purement-mocke',
      listen_path: '',
      real_target_url: '',
      is_mocked: true,
      rewrite_directory_urls: false,
      rules: [],
    };
    const { getByRole, queryByLabelText } = render(ServiceForm, {
      props: { service: existingService, isEdit: true },
    });

    expect(getByRole('switch', { name: PURELY_MOCKED_LABEL })).toHaveAttribute('aria-checked', 'true');
    expect(queryByLabelText('Real target URL')).not.toBeInTheDocument();
  });

  it('decocher reaffiche le champ cible sans perte des regles existantes', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const existingRules = [{ name: 'r1', action: 'mock' }];
    const existingService = {
      name: 'deja-purement-mocke',
      listen_path: '',
      real_target_url: '',
      is_mocked: true,
      rewrite_directory_urls: false,
      rules: existingRules,
    };
    const { getByLabelText, getByRole, container } = render(ServiceForm, {
      props: { service: existingService, isEdit: true, onSave },
    });

    await fireEvent.click(getByRole('switch', { name: PURELY_MOCKED_LABEL }));
    const targetInput = getByLabelText('Real target URL');
    await setInput(targetInput, 'http://nouvelle-cible:8080');
    await submitForm(container);

    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        real_target_url: 'http://nouvelle-cible:8080',
        rules: existingRules,
      }),
    );
  });

  it("bascule a posteriori : avertit sans bloquer quand des regles Proxy existent deja, 'Enregistrer quand meme' sauvegarde", async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const existingService = {
      name: 'avec-regles-proxy',
      listen_path: '',
      real_target_url: 'http://backend:8080',
      is_mocked: true,
      rewrite_directory_urls: false,
      rules: [
        { name: 'proxy-rule', action: 'proxy' },
        { name: 'mock-rule', action: 'mock' },
      ],
    };
    const { getByRole, container, queryByTestId, getByTestId } = render(ServiceForm, {
      props: { service: existingService, isEdit: true, onSave },
    });

    await fireEvent.click(getByRole('switch', { name: PURELY_MOCKED_LABEL }));
    await submitForm(container);

    expect(onSave).not.toHaveBeenCalled();
    const warning = getByTestId('service-form-purely-mocked-warning');
    expect(warning).toHaveTextContent('proxy-rule');
    expect(warning).not.toHaveTextContent('mock-rule');

    await fireEvent.click(getByTestId('service-form-purely-mocked-save-anyway-button'));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ real_target_url: '', is_mocked: true }));
    expect(queryByTestId('service-form-purely-mocked-warning')).not.toBeInTheDocument();
  });

  it("bascule a posteriori : 'Revenir en arriere' referme l'avertissement sans sauvegarder", async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const existingService = {
      name: 'avec-regle-proxy',
      listen_path: '',
      real_target_url: 'http://backend:8080',
      is_mocked: true,
      rewrite_directory_urls: false,
      rules: [{ name: 'proxy-rule', action: 'proxy' }],
    };
    const { getByRole, container, getByTestId, queryByTestId } = render(ServiceForm, {
      props: { service: existingService, isEdit: true, onSave },
    });

    await fireEvent.click(getByRole('switch', { name: PURELY_MOCKED_LABEL }));
    await submitForm(container);
    expect(getByTestId('service-form-purely-mocked-warning')).toBeInTheDocument();

    await fireEvent.click(getByTestId('service-form-purely-mocked-cancel-button'));
    expect(onSave).not.toHaveBeenCalled();
    expect(queryByTestId('service-form-purely-mocked-warning')).not.toBeInTheDocument();
  });

  it("aucun avertissement quand le service purement mocke n'a aucune regle Proxy", async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const existingService = {
      name: 'sans-regle-proxy',
      listen_path: '',
      real_target_url: 'http://backend:8080',
      is_mocked: true,
      rewrite_directory_urls: false,
      rules: [{ name: 'mock-rule', action: 'mock' }],
    };
    const { getByRole, container } = render(ServiceForm, {
      props: { service: existingService, isEdit: true, onSave },
    });

    await fireEvent.click(getByRole('switch', { name: PURELY_MOCKED_LABEL }));
    await submitForm(container);

    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ real_target_url: '', is_mocked: true }));
  });
});

describe('ServiceForm: settings the form does not show', () => {
  const soapService = (wsdl_mode) => ({
    name: 'soap-svc',
    listen_path: '',
    real_target_url: 'http://backend:8080',
    is_mocked: true,
    rewrite_directory_urls: true,
    group_name: null,
    wsdl_mode,
    rules: [],
  });

  it.each(['mock', 'proxy'])('keeps a WSDL mode set to %s when the service is saved', async (wsdl_mode) => {
    const onSave = vi.fn();
    const { container } = render(ServiceForm, { props: { service: soapService(wsdl_mode), isEdit: true, onSave } });

    await submitForm(container);
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ wsdl_mode }));
  });

  it('gives a new service the automatic WSDL mode', async () => {
    const onSave = vi.fn();
    const { container } = render(ServiceForm, { props: { onSave } });

    await setInput(container.querySelector('[data-testid="service-form-name-input"]'), 'new-svc');
    await setInput(container.querySelector('[data-testid="service-form-target-input"]'), 'http://backend:8080');
    await submitForm(container);
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ wsdl_mode: 'auto' }));
  });
});
