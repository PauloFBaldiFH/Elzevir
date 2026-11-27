# Conexões e planos de nuvem — P9/P10

Conexões são institucionais, identificadas por ID aleatório, com um arquivo
rclone privado por conexão. Não importam nem sobrescrevem a configuração global
do usuário. A lista de providers e os formulários vêm de `config providers` e
da máquina de estados `config update --all --non-interactive` do executável
verificado. Campos sensíveis são mascarados; o estado completo e tokens ficam
somente em arquivos privados. Não mostra ajuda dinâmica que possa incluir
segredos em comandos de autorização. Perguntas opcionais aceitam o padrão.

Respostas usam ambiente privado do subprocesso (`RCLONE_STATE`/`RCLONE_RESULT`),
com limpeza de variáveis rclone herdadas e nível NOTICE para permitir o endereço de autorização, sem ativar debug. Nunca argumentos nem
shell. Senhas de opções `IsPassword` são obscurecidas via stdin antes de
responder, como o configurador nativo. Obscure não substitui permissões privadas
nem a cifra age do backup. OAuth usa o callback local do rclone; o painel mostra
somente o link local `/auth` na porta 53682. Tokens recebidos não entram no
histórico. Fluxos interrompidos preservam estado para repetir a resposta.

Crypt é outra conexão gerenciada usando o provider `crypt` e referência a uma
conexão pronta. Segredos seguem o mesmo formulário. Um teste envia um arquivo
fictício com nome aleatório para subdiretório específico e só remove o próprio
arquivo de teste depois de verificar o envio. Nunca usa sync nem purge.

Planos definem ID, conexão pronta, subdiretório relativo sem travessia, retenção
de 1 a 365, horário diário e fuso IANA. Agendas desativadas não executam nada.
O trabalhador faz snapshot cifrado, envia por copyto e verifica com check
(download em streaming quando necessário). Só então registra o artefato remoto
como concluído e aplica retenção dos IDs pertencentes ao plano. Falha remota
não remove a cópia local nem promove sucesso no histórico. Arquivos estranhos
no remoto não são candidatos a retenção. Um mesmo plano nunca executa duas
vezes por data civil, inclusive em mudança de fuso/relógio ou reinício.

O configurador e as transferências rodam exclusivamente na conta limitada do
servidor. O cliente elevado instala o executável e envia intenções pelo IPC;
não executa rclone sobre configurações graváveis pelo daemon. O IPC admite
frames de até 32 KiB e respostas de 64 KiB. Respostas privadas ficam em arquivo
0600 até o trabalho começar, são consumidas antes da execução e nunca entram no
histórico. Uma interrupção descarta entradas pendentes e pede nova resposta.
## Ambiente herdado no Windows

A limpeza de variáveis herdadas de rclone ignora maiúsculas/minúsculas no
prefixo RCLONE_, pois o Windows trata nomes de ambiente dessa forma. Assim,
flags de depuração ou configuração com grafia mista não contornam a limpeza.

A descoberta admite nomes técnicos compostos como `google cloud storage` quando constam do catálogo do binário rclone. O nome tem até 64 bytes e cada palavra contém apenas letras ASCII minúsculas, números e sublinhado. Um espaço separa cada palavra. A configuração envia o nome como um único argumento ao executável. Nomes de opções continuam sujeitos à validação mais restrita.
