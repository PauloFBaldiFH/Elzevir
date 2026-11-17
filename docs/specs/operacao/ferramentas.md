# Ferramentas opcionais e integrações — P5/P6/P7/P9/P10/P11

`cloudflared` e `rclone` são instalados somente por ação explícita do operador.
Cada instalação consulta a release oficial uma vez, fixa sua tag e seleciona
exatamente o ativo da arquitetura do host. Exige digest SHA-256 do ativo na API
oficial do GitHub, valida o download antes de extrair/executar, limita tamanhos
e recusa ativos sem checksum. Um arquivo existente sem registro reconhecido é
preservado. O registro contém versão/hash; não contém credenciais.

Os executáveis ficam em diretório de programas controlado pelo administrador,
fora dos dados graváveis pela conta do daemon. Cada execução confere o registro
e hash do arquivo. Ferramentas não são procuradas em PATH gravável para execução
privilegiada. Instalação/substituição é serializada por trava de ferramenta.
Execuções de rclone mantêm trava compartilhada desde a validação do hash até o
fim do processo; atualização exige trava exclusiva e recusa ferramenta em uso.
A publicação mantém diário privado no diretório de programas. Repetir a
instalação recupera a troca interrompida: conclui o registro para um executável
novo conhecido ou repõe o anterior se o destino estiver ausente. Executáveis
com hash desconhecido são preservados e recusados.
O hash antes e depois da verificação de versão do executável estagiado deve
coincidir.
Segredos são arquivos privados dentro de `DATA_DIR/secrets`; nunca argumentos,
logs, histórico de trabalhos nem arquivos de suporte. Executores usam argv,
entrada privada, captura limitada, timeout e encerramento/reap de subprocessos.

Domínio próprio é opcional e pode ser informado depois da instalação. Os
hostnames do catálogo e da equipe entram na ação explícita de publicação; não
há domínio padrão a registrar. Antes dessa ação, a instalação permanece local.
Escolher o domínio depois não exige recompilar o Elzevir. A ativação pública
exige autorização da conta Cloudflare, configuração de DNS e teste HTTPS real.

Túnel com domínio próprio exige configuração de equipe por hostname diferente
do catálogo; a borda do Elzevir recusa equipe pelo hostname do catálogo. Origem
é a porta efetiva configurada, nunca um fallback às portas do Koha. Login exige
interação na conta do operador e só o endereço autorizado aparece na UI. O
config de cloudflared é gerado de dados tipados (JSON compatível com YAML).
Teste e log são projeções limitadas, sem conteúdo de tokens/certificados.

Endereço gratuito consulta `/v1/capabilities` antes de pedir inscrição. Sem
suporte explícito a `elzevir` e origem configurável, recusa cadastro e informa a
dependência B1; não tenta publicar como Koha. Assinaturas seguem KEI-SIG-v1,
Ed25519, relógio UTC e nonce aleatório. Recuperação inclui chave somente em
arquivo privado explicitamente exportado. Credenciais de equipe remota,
listagem pública, renovação e desistência têm confirmação própria. QR contém
somente endereço público validado, nunca token ou chave.

A configuração de nuvem deriva das opções de `rclone config providers`, com
limites de campos e tamanhos. Segredos usam entrada privada e arquivos; o
processo de OAuth entrega endereço de autorização ao operador e guarda a
resposta apenas em configuração privada. Crypt é opcional, além da cifra age
do próprio artefato. Cada plano tem identidade, remoto, subdiretório, horário,
fuso e retenção; envio de teste usa arquivo fictício. Retenção remota só remove
nomes de artefatos gerados pelo Elzevir depois de um envio confirmado. Nunca
usa sync/delete sobre o remoto inteiro. Histórico e agendamento pertencem ao
trabalhador do servidor; fechar o painel não cancela um envio aceito.

A entrada e a gestão da conta Cloudflare exigem terminal elevado. O certificado
da conta fica privado ao operador (Unix 0600; Windows ACL de Administradores e
SYSTEM, sem herdar acesso do serviço). O daemon só usa as credenciais do túnel.
O certificado não é transportado por backups automáticos; recuperar o conector
não entrega acesso à gestão da conta. A UI mostra somente a URL de autorização.

O login mantém o certificado de gestão exclusivamente no perfil do operador
(`~/.cloudflared/cert.pem` ou `%USERPROFILE%\.cloudflared\cert.pem`). O diretório
da instituição recebe apenas a credencial limitada ao túnel; uma restauração
não concede gestão da conta e exige novo login antes de alterar DNS.

Atualizar cloudflared para o conector selecionado observa o serviço antes de
instalar. Se estiver em execução, para e tenta reiniciá-lo também quando a
instalação falha. Um conector parado permanece parado. A operação não para
conectores de outras instituições; se outro processo impedir a troca do
executável compartilhado, a instalação falha preservando o anterior.
