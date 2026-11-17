# Domínio e túnel Elzevir — P6/P7

O serviço padrão é https://broker.elzevir.app e o domínio solicitado é
elzevir.app. A CLI aceita --broker/--broker-url; a variável ELZEVIR_BROKER_URL
substitui o padrão quando não há flag. No painel, URL vazia usa ambiente/padrão.
A escolha do endpoint é persistida com a identidade: mudar o ambiente não troca
silenciosamente o serviço de uma inscrição existente. Instalar/iniciar o
Elzevir não registra biblioteca, DNS ou túnel automaticamente.

## Registro

O cliente não exige /v1/capabilities nem o anúncio B1 do protótipo anterior.
POST /v1/device/start envia institution_name, contact_email, requested_name
quando preenchido e domain=elzevir.app; CNPJ é omitido. Não exige campos
extras ou extensões do broker anterior. A origem HTTP loopback e a porta
configurada continuam registradas localmente para validar retomada e recuperação.
A resposta informa status, domain, suggested_slug, expires_in e interval;
verification_uri e hostnames são opcionais. Se approved, segue direto para
enroll. Se pending, espera interval antes de cada POST /v1/device/poll;
respeita um novo interval válido e termina em approved, denied ou expired.
Polling e provisionamento têm prazo total limitado e relógio substituível nos
testes. Consumed continua aceito para repetir enroll após perda de resposta.

POST /v1/enroll envia device_code e chave pública Ed25519 raw de 32 bytes em
Base64 padrão; aceita 202 com library_id, job_id, domain e hostnames. Antes de
qualquer efeito no host, valida domain=elzevir.app e o par de nomes
<slug>.elzevir.app / <slug>-admin.elzevir.app. O cliente não publica Elzevir
como Koha e não aceita os dois usos no mesmo hostname.

A chave é gerada localmente e salva antes de enviar a inscrição. A configuração
privada em DATA_DIR/secrets/broker.json contém private_key (seed Ed25519 de
32 bytes em Base64), library_id, hostnames, domínio, URL e progresso. Lê também
os nomes seed/library/hosts de configurações anteriores. IDs são persistidos
antes de esperar GET /v1/jobs/<id>; repetir uma operação retoma o trabalho em
vez de gerar outra identidade. Configurações antigas sem domínio continuam
legíveis, sem serem convertidas silenciosamente para outro endereço.

## Assinaturas e operações

Os quatro headers X-KEI-Library/Timestamp/Nonce/Signature seguem KEI-SIG-v1:
método maiúsculo, path/query exato, segundos UTC, nonce URL-safe e SHA-256 hex
do corpo, separados por newline sem newline final. GET/DELETE sem corpo
assinam os bytes vazios. Respostas de operações sem payload aceitam 204.

GET /v1/jobs/<id> espera done; failed preserva a identidade e apresenta erro
classificado, sem copiar tokens ou mensagens remotas livres para a interface.
GET /v1/tunnel-token fornece o token ao cloudflared por arquivo privado
(token-file), equivalente à opção --token sem expor o segredo em argv.
GET /v1/library observa estado, hostnames, staff_remote_access e
staff_allow_cidrs; campos opcionais como services/public_listing não são
pré-requisitos do novo contrato.

PUT/DELETE /v1/staff-credentials gerencia acesso remoto independentemente da
senha local. Senhas têm 12–256 caracteres e entrada mascarada; CIDRs são
validados. POST /v1/rotate busca novo token e reconecta; POST /v1/heartbeat
envia installer_version. DELETE /v1/library para o conector e desprovisiona;
só depois da confirmação remota limpa o vínculo local, permitindo novo cadastro.
URL HTTPS, corpos, prazos, nonce, nomes e arquivos têm limites explícitos.

## Painel e recuperação

A seção Endereços apresenta o fluxo Domínio & Túnel (*.elzevir.app), com ações
separadas de registrar/retomar, atualizar estado, configurar/desativar equipe,
rotacionar e desvincular. Mostra fase de autorização/provisionamento/conexão,
endereços atribuídos, observação remota e estado local do serviço cloudflared.
Endereço atribuído/configurado não é prova de disponibilidade pública: os
resultados do teste HTTPS continuam separados. Abrir a tela não chama o broker. Sem observação do gerenciador local, o estado
do conector é desconhecido, inclusive antes do primeiro cadastro.

Exportar a identidade exige arquivo privado novo. Recuperação valida domínio,
biblioteca e chave antes de persistir; renova o token e permite repetir após
falha. QR representa somente a URL pública do catálogo. A validação do contrato
usa respostas simuladas, sem criar conta, DNS, túnel ou inscrição de verdade.
