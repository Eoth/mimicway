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
  it('rejects an empty name', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), '');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('The service name is required.');
  });

  it('rejects the reserved name "api"', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'api');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('The name "api" is reserved by Mimicway');
  });

  it('rejects the reserved name "index.html"', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'index.html');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('The name "index.html" is reserved by Mimicway');
  });

  it('accepts an empty listen_path (catch-all)', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, container } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'my-svc');
    await setInput(getByLabelText('Listen path (optional)'), '');
    await setInput(getByLabelText('Real target URL'), 'http://backend:8080');
    await submitForm(container);
    expect(onSave).toHaveBeenCalled();
  });

  it('accepts the listen_path "/" (catch-all)', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, container } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'my-svc');
    await setInput(getByLabelText('Listen path (optional)'), '/');
    await setInput(getByLabelText('Real target URL'), 'http://backend:8080');
    await submitForm(container);
    expect(onSave).toHaveBeenCalled();
  });

  it('accepts the listen_path "/*" (explicit catch-all)', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, container } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'my-svc');
    await setInput(getByLabelText('Listen path (optional)'), '/*');
    await setInput(getByLabelText('Real target URL'), 'http://backend:8080');
    await submitForm(container);
    expect(onSave).toHaveBeenCalled();
  });

  it('accepts a listen_path with a sub-path', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, container } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'my-svc');
    await setInput(getByLabelText('Listen path (optional)'), '/v1/users/*');
    await setInput(getByLabelText('Real target URL'), 'http://backend:8080');
    await submitForm(container);
    expect(onSave).toHaveBeenCalled();
  });

  it('rejects a name with a /', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'my/svc');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('A service name cannot contain a path separator (/ or \\).');
  });

  it('submits a name that already exists (the backend checks uniqueness within a group)', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, container } = render(ServiceForm, {
      props: { onSave, existingNames: ['existing-svc'] },
    });

    await setInput(getByLabelText('Service name'), 'existing-svc');
    await setInput(getByLabelText('Real target URL'), 'http://backend:8080');
    await submitForm(container);
    expect(onSave).toHaveBeenCalled();
  });

  it('accepts the same name when editing', async () => {
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

  it('disables the name input when editing', async () => {
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

  it('keeps the name input editable when cloning (a prefilled service without isEdit)', async () => {
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

  it('rejects a name with a space', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'my svc');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent(
      'A service name can only contain letters, digits, dashes (-) and underscores (_).',
    );
  });

  it('rejects a name with a special character', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'svc@name!');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent(
      'A service name can only contain letters, digits, dashes (-) and underscores (_).',
    );
  });

  it('accepts a name with underscores, dashes and digits', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, container } = render(ServiceForm, { props: { onSave } });

    await setInput(getByLabelText('Service name'), 'svc_v2-42');
    await setInput(getByLabelText('Real target URL'), 'http://backend:8080');
    await submitForm(container);
    expect(onSave).toHaveBeenCalled();
  });
});

describe('ServiceForm: purely mocked service', () => {
  it('turning the switch on hides the target input and saves without a target', async () => {
    const onSave = vi.fn().mockResolvedValue({});
    const { getByLabelText, getByRole, queryByLabelText, container } = render(ServiceForm, { props: { onSave } });

    // Off by default, so the target input shows.
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

  it('opens an existing service without a target with the switch on', () => {
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

  it('turning the switch off shows the target input again and keeps the rules', async () => {
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

  it('switching on with Proxy rules holds the save behind a warning that lists them; "Save anyway" saves', async () => {
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

  it('switching on with Proxy rules: "Go back" closes the warning without saving', async () => {
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

  it('saves without a warning when the purely mocked service has no Proxy rule', async () => {
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
