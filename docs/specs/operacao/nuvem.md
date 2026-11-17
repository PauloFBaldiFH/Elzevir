# Conexões e planos de nuvem — P9/P10

Conexões são institucionais, identificadas por ID aleatório, com um arquivo
rclone privado por conexão. Não importam nem sobrescrevem a configuração global
do usuário. A lista de providers e os formulários vêm de `config providers` e
da máquina de estados `config update --all --non-interactive` do executável
verificado. Campos sensíveis são mascarados; o estado completo e tokens ficam
somente em arquivos privados. Não mostra ajuda dinâmica que possa incluir
segredos em comandos de autorização. Perguntas opcionais aceitam o padrão.

Respostas usam ambiente privado do subprocesso (`RCLONE_STATE`/`RCLONE_RESULT`),
com limpeza de variáveis rclone herdadas e nível ERROR. Nunca argumentos nem
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
