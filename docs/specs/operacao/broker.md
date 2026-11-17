# Endereço gratuito e recuperação — P6/P7

O domínio público pode ser definido depois da instalação. Não há URL de broker
nem sufixo de domínio embutidos no cliente: a URL entra na inscrição e os
hostnames do catálogo e da equipe vêm da resposta validada do serviço. Sem
inscrição, o Elzevir continua funcionando localmente; não cria identidade,
DNS ou túnel automaticamente. A ausência de domínio não bloqueia a instalação,
o painel, os backups nem os agendamentos.

Para ativar o endereço gratuito posteriormente, é necessário disponibilizar um
broker compatível com o contrato abaixo, configurar seu domínio/DNS e informar
sua URL HTTPS na inscrição. Essa etapa requer teste real de provisionamento e
acesso público; testes locais do cliente não comprovam a implantação externa.

O operador informa a URL HTTPS do broker. O cliente recusa redirecionamentos,
credenciais na URL, query/fragmento e caminhos de base. Antes de enviar nome ou
contato, exige `services` contendo `elzevir` e a capacidade versionada
`elzevir: {contract: 1, configurable_origin: true}`. Sem isso, informa B1 e não
cria inscrição. Esse contrato exige implementação e implantação no repositório
do broker; a integração local não ativa um serviço externo automaticamente.

`POST /v1/device/start` recebe os campos KEI existentes e `origin`, a URL HTTP
loopback com porta real do Elzevir. O broker deve persistir essa origem junto
à inscrição e gerar ingress para os hostnames `opac` e `staff`. Não usa portas
padrão do Koha. Aprovação usa `/v1/device/poll`; inscrição envia chave pública
Ed25519 raw de 32 bytes em Base64 padrão. O fluxo mantém uma identidade privada
antes da inscrição e guarda library/job IDs antes de esperar provisionamento.

Requisições posteriores assinam exatamente `KEI-SIG-v1`, método maiúsculo,
caminho e query, segundos UTC, nonce aleatório e SHA-256 do corpo, separados
por newline e sem newline final. Assinaturas usam implementação criptográfica
existente. Timeout, corpo e repetição são limitados. Respostas e mensagens
remotas não entram nos logs nem no histórico. Falhas não publicam endereços
como testados e não substituem identidade existente.

Só um conector gerenciado por instituição. O modo por hostname é validado no
servidor antes de iniciar o túnel; o token fica em arquivo privado, nunca em
argv. Um batimento por hora pertence ao trabalhador do servidor. A resposta do
broker é uma observação; não modifica a senha local da equipe.

Listagem, credenciais da borda, renovar token e desistir exigem intenções
separadas. Senha remota usa formulário mascarado ou prompt privado, sem opção
CLI com texto da senha. Faixas CIDR são validadas e limitadas a vinte.

Recuperação exporta JSON versionado com identidade Ed25519, biblioteca e URL
em arquivo novo privado. Importação valida a capacidade, a assinatura em
`GET /v1/library` e os hostnames antes de persistir a identidade, e renova o
token. Não executa conteúdo do arquivo. Se a renovação falhar, mantém os dados
para repetir a operação e informa falha; não declara exclusão do host anterior.
QR representa exclusivamente a URL pública do catálogo.

B1 também deve tornar `/v1/enroll` idempotente para a mesma dupla
`device_code`/chave pública, devolvendo os mesmos IDs após perda da resposta.
O cliente repete a inscrição quando o poll informa `consumed`; outra chave
continua recusada. A capacidade de contrato 1 só deve ser anunciada depois de
validar origem, idempotência e a proteção/listagem de equipe para Elzevir.
