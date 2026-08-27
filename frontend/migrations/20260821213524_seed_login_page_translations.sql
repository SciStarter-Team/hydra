-- Spanish and Portuguese for the login page.
--
-- The ids are sha256(context || 0x1f || source), the same derivation
-- i18n::phrase_id performs, so these rows are the same phrases the site
-- discovers for itself. Change a source string in the template and its id
-- changes with it: the seeded translation is then simply unused, and the new
-- wording turns up in the admin as untranslated.
--
-- Seeding fills blanks only. A phrase already carrying a translation is left
-- alone, so re-running this can never overwrite a translator's work.

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('7bbf86b18c761d90586d7679580a4e70bd5bd00efc235177e19bc92210df8313', '', 'Log In or Sign Up', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('7bbf86b18c761d90586d7679580a4e70bd5bd00efc235177e19bc92210df8313', 'es', 'Iniciar sesión o crear una cuenta', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('7bbf86b18c761d90586d7679580a4e70bd5bd00efc235177e19bc92210df8313', 'pt', 'Entrar ou criar uma conta', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('2327130ec7650aa9ab0145b318209057cdd8954a870ab3a94e1f9a047946f7e3', '', 'You''re not logged in to an account which is authorized to access that resource. If you have a different account which might be allowed, try logging in to that account now.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('2327130ec7650aa9ab0145b318209057cdd8954a870ab3a94e1f9a047946f7e3', 'es', 'No has iniciado sesión con una cuenta autorizada para acceder a ese recurso. Si tienes otra cuenta que sí pueda tener acceso, inicia sesión con ella ahora.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('2327130ec7650aa9ab0145b318209057cdd8954a870ab3a94e1f9a047946f7e3', 'pt', 'Você não está conectado a uma conta autorizada a acessar esse recurso. Se você tiver outra conta que possa ter acesso, entre com ela agora.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('88b5524a996babac4675bc94df578cc37aae69ef5d99ebde1cc5e1f1cbda4bbe', '', 'Something went wrong on our end. It''s not your fault. Please wait a little while and then try again.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('88b5524a996babac4675bc94df578cc37aae69ef5d99ebde1cc5e1f1cbda4bbe', 'es', 'Algo falló de nuestro lado. No es culpa tuya. Espera un momento y vuelve a intentarlo.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('88b5524a996babac4675bc94df578cc37aae69ef5d99ebde1cc5e1f1cbda4bbe', 'pt', 'Algo deu errado do nosso lado. A culpa não é sua. Aguarde um pouco e tente novamente.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('f147169d722c0842a8858ed5f20c06b6c63e873789e192cd973057906088f7a5', '', 'That password was incorrect.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('f147169d722c0842a8858ed5f20c06b6c63e873789e192cd973057906088f7a5', 'es', 'Esa contraseña es incorrecta.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('f147169d722c0842a8858ed5f20c06b6c63e873789e192cd973057906088f7a5', 'pt', 'Essa senha está incorreta.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('a88797b7b9f845f82eb028f9c0598844ac3291ce314d1ec0123c51e7a5ab247b', '', 'That email address already belongs to a SciStarter account. Log in below to continue.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('a88797b7b9f845f82eb028f9c0598844ac3291ce314d1ec0123c51e7a5ab247b', 'es', 'Esa dirección de correo electrónico ya pertenece a una cuenta de SciStarter. Inicia sesión abajo para continuar.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('a88797b7b9f845f82eb028f9c0598844ac3291ce314d1ec0123c51e7a5ab247b', 'pt', 'Esse e-mail já pertence a uma conta do SciStarter. Entre abaixo para continuar.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('47db7218735ae497b323d4c0e0a277a8de920c1e7658fe351710a0bc91e40042', '', 'Too many signup attempts from your network right now. Please try again in an hour.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('47db7218735ae497b323d4c0e0a277a8de920c1e7658fe351710a0bc91e40042', 'es', 'Se han hecho demasiados intentos de registro desde tu red en este momento. Inténtalo de nuevo en una hora.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('47db7218735ae497b323d4c0e0a277a8de920c1e7658fe351710a0bc91e40042', 'pt', 'Muitas tentativas de cadastro a partir da sua rede neste momento. Tente novamente em uma hora.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('c6417f8eb001eec593e5a97e5ab88f31c5a016165d5678e227ee7c57610ebcee', '', 'Log In', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('c6417f8eb001eec593e5a97e5ab88f31c5a016165d5678e227ee7c57610ebcee', 'es', 'Iniciar sesión', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('c6417f8eb001eec593e5a97e5ab88f31c5a016165d5678e227ee7c57610ebcee', 'pt', 'Entrar', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('ac02ebe27488f0c51bb49d15c03059e5308e2a748a26d76c74630d0f95f494cd', '', 'Sign Up', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('ac02ebe27488f0c51bb49d15c03059e5308e2a748a26d76c74630d0f95f494cd', 'es', 'Crear cuenta', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('ac02ebe27488f0c51bb49d15c03059e5308e2a748a26d76c74630d0f95f494cd', 'pt', 'Criar conta', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('f03a1637aae59b2380757afea37bb40d2cdde878654d8bf89cc5faddfd61d73d', '', 'Email', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('f03a1637aae59b2380757afea37bb40d2cdde878654d8bf89cc5faddfd61d73d', 'es', 'Correo electrónico', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('f03a1637aae59b2380757afea37bb40d2cdde878654d8bf89cc5faddfd61d73d', 'pt', 'E-mail', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('7421d5dd3b0bc886911d47a5487b6413814a71391a1de119c5552ad0a5166436', '', 'Password', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('7421d5dd3b0bc886911d47a5487b6413814a71391a1de119c5552ad0a5166436', 'es', 'Contraseña', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('7421d5dd3b0bc886911d47a5487b6413814a71391a1de119c5552ad0a5166436', 'pt', 'Senha', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('dbc84a83098c35b330fa0bdeb201d9c54159ea7f2c9d67f4f9ea577e634ad311', '', 'Handle', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('dbc84a83098c35b330fa0bdeb201d9c54159ea7f2c9d67f4f9ea577e634ad311', 'es', 'Nombre de usuario', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('dbc84a83098c35b330fa0bdeb201d9c54159ea7f2c9d67f4f9ea577e634ad311', 'pt', 'Nome de usuário', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('65a5b7b48b6fcb93f767d42537bcf0045566181be298b3d62273e4a9a0b009d4', '', 'Re-type Password', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('65a5b7b48b6fcb93f767d42537bcf0045566181be298b3d62273e4a9a0b009d4', 'es', 'Repite la contraseña', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('65a5b7b48b6fcb93f767d42537bcf0045566181be298b3d62273e4a9a0b009d4', 'pt', 'Repita a senha', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('a2e6743e9b365d703e8f2ca69e3cc547f771e909e724def0388ea867e5606530', '', 'Zip/Postal Code', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('a2e6743e9b365d703e8f2ca69e3cc547f771e909e724def0388ea867e5606530', 'es', 'Código postal', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('a2e6743e9b365d703e8f2ca69e3cc547f771e909e724def0388ea867e5606530', 'pt', 'CEP/Código postal', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('3e812c35bcfb14833c272539d2fabba429b833d43f865d975348b615e2bfc776', '', 'Use passkey instead of password', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('3e812c35bcfb14833c272539d2fabba429b833d43f865d975348b615e2bfc776', 'es', 'Usar una clave de acceso en lugar de contraseña', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('3e812c35bcfb14833c272539d2fabba429b833d43f865d975348b615e2bfc776', 'pt', 'Usar chave de acesso em vez de senha', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('0a4ac1800b8d90cf3e333229e887cbde42d53574120fe68d32094be44a6e111d', '', 'You are signing up for a protected account.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('0a4ac1800b8d90cf3e333229e887cbde42d53574120fe68d32094be44a6e111d', 'es', 'Estás creando una cuenta protegida.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('0a4ac1800b8d90cf3e333229e887cbde42d53574120fe68d32094be44a6e111d', 'pt', 'Você está criando uma conta protegida.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('c3e6170029739fdbfd06d74ca7e731b4b186f32a0d8d9eba7f06515f2e2b3b0b', '', 'Sign up for our newsletter', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('c3e6170029739fdbfd06d74ca7e731b4b186f32a0d8d9eba7f06515f2e2b3b0b', 'es', 'Suscribirme al boletín', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('c3e6170029739fdbfd06d74ca7e731b4b186f32a0d8d9eba7f06515f2e2b3b0b', 'pt', 'Assinar nosso boletim', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('10c1cc719a4e25042bd5f7bfb0e33fcaaa46bccec09e39e8cdcedde8849ea0bb', '', 'We couldn''t run our automatic spam check on your browser or network. You can still sign up—we''ll email you a link to finish creating your account.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('10c1cc719a4e25042bd5f7bfb0e33fcaaa46bccec09e39e8cdcedde8849ea0bb', 'es', 'No pudimos ejecutar nuestra comprobación automática contra el spam en tu navegador o en tu red. Aun así puedes crear tu cuenta: te enviaremos por correo un enlace para terminar de crearla.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('10c1cc719a4e25042bd5f7bfb0e33fcaaa46bccec09e39e8cdcedde8849ea0bb', 'pt', 'Não conseguimos executar nossa verificação automática contra spam no seu navegador ou na sua rede. Você ainda pode criar sua conta: enviaremos por e-mail um link para concluir o cadastro.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('c31678b9cdf1f44bc861fa09cc59a79303a6f9c8ee30caf44d962139c794557a', '', 'Please enter a password', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('c31678b9cdf1f44bc861fa09cc59a79303a6f9c8ee30caf44d962139c794557a', 'es', 'Escribe una contraseña', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('c31678b9cdf1f44bc861fa09cc59a79303a6f9c8ee30caf44d962139c794557a', 'pt', 'Digite uma senha', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('133e7803022df58fcc03bf8c962dbae3f77db3d2d581cb9cb6a8baea6ce3aaec', '', 'Something went wrong. Please try again.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('133e7803022df58fcc03bf8c962dbae3f77db3d2d581cb9cb6a8baea6ce3aaec', 'es', 'Algo salió mal. Inténtalo de nuevo.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('133e7803022df58fcc03bf8c962dbae3f77db3d2d581cb9cb6a8baea6ce3aaec', 'pt', 'Algo deu errado. Tente novamente.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('50bc7d7ec272d9e0fb877c4d91ba9aa6354087cfb8c27885e2ed9c6651ef2315', '', 'Passkey setup could not be started.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('50bc7d7ec272d9e0fb877c4d91ba9aa6354087cfb8c27885e2ed9c6651ef2315', 'es', 'No se pudo iniciar la configuración de la clave de acceso.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('50bc7d7ec272d9e0fb877c4d91ba9aa6354087cfb8c27885e2ed9c6651ef2315', 'pt', 'Não foi possível iniciar a configuração da chave de acesso.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('97a48f78f6579fc451498f82aac8f0ecb8c5d537d0abba9e17c9e30c957b2d15', '', 'Sign in failed.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('97a48f78f6579fc451498f82aac8f0ecb8c5d537d0abba9e17c9e30c957b2d15', 'es', 'No se pudo iniciar sesión.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('97a48f78f6579fc451498f82aac8f0ecb8c5d537d0abba9e17c9e30c957b2d15', 'pt', 'Não foi possível entrar.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('75feef84abb2e14fe1408d6ce988582f596a90b92d3d901ef2723fbea247b6a7', '', 'Passkey sign in failed.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('75feef84abb2e14fe1408d6ce988582f596a90b92d3d901ef2723fbea247b6a7', 'es', 'No se pudo iniciar sesión con la clave de acceso.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('75feef84abb2e14fe1408d6ce988582f596a90b92d3d901ef2723fbea247b6a7', 'pt', 'Não foi possível entrar com a chave de acesso.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('315cda7b35ceb61bdf00a3226296bdabe9831fb5253f02538f8579b83cc64a5e', '', 'Passkey signup failed.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('315cda7b35ceb61bdf00a3226296bdabe9831fb5253f02538f8579b83cc64a5e', 'es', 'No se pudo crear la cuenta con la clave de acceso.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('315cda7b35ceb61bdf00a3226296bdabe9831fb5253f02538f8579b83cc64a5e', 'pt', 'Não foi possível criar a conta com a chave de acesso.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';


INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('5c8f3807c821545675469c48f33ebd748b40f994d7040e32564cef4cd8ca3dd7', '', 'Unknown error', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('5c8f3807c821545675469c48f33ebd748b40f994d7040e32564cef4cd8ca3dd7', 'es', 'Error desconocido', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('5c8f3807c821545675469c48f33ebd748b40f994d7040e32564cef4cd8ca3dd7', 'pt', 'Erro desconhecido', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

