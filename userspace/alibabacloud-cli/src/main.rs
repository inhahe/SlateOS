#![deny(clippy::all)]

//! alibabacloud-cli — OurOS Alibaba Cloud (Aliyun, China's #1 cloud, AI/Qwen, Hangzhou, NYSE:BABA)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_aliyun(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: alibabacloud [OPTIONS]");
        println!("Alibaba Cloud (OurOS) — China's largest cloud (Aliyun), Qwen AI, parent NYSE:BABA");
        println!();
        println!("Options:");
        println!("  --ecs                  Elastic Compute Service (Alibaba VMs)");
        println!("  --oss                  Object Storage Service (S3-compatible)");
        println!("  --pai                  PAI (Platform for AI) + Qwen LLM family");
        println!("  --polardb              PolarDB (cloud-native distributed database)");
        println!("  --maxcompute           MaxCompute (cloud data warehouse, BigQuery-like)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Alibaba Cloud 2024 (OurOS) — aliyun CLI 3.x"); return 0; }
    println!("Alibaba Cloud 2024 (OurOS) — China's #1 Cloud Provider (Aliyun)");
    println!("  Vendor: Alibaba Cloud (Aliyun), subsidiary of Alibaba Group (Hangzhou, China)");
    println!("          Parent: Alibaba Group Holding Ltd (NYSE:BABA, HKEX:9988)");
    println!("  Founders: Jack Ma (Ma Yun) + Joseph Tsai + 16 others, Alibaba founded 1999");
    println!("          Aliyun (Alibaba Cloud) founded 2009 by Wang Jian (CTO)");
    println!("          Jack Ma: stepped down as chairman 2019, lower profile post-2020");
    println!("          Daniel Zhang: led Aliyun 2022-2023, departed after Ant Group regulatory issues");
    println!("          Eddie Wu: CEO of Alibaba Group 2023+");
    println!("          Joseph Tsai: Chairman of Alibaba Group 2023+");
    println!("  Public market (parent NYSE:BABA / HKEX:9988):");
    println!("         Alibaba IPO Sept 2014 — largest IPO in history at the time ($25B)");
    println!("         Hong Kong dual-listing Nov 2019");
    println!("         peak ~$310 (Oct 2020) — collapsed after Ant Group IPO blocked Nov 2020");
    println!("         settled $70-100 range 2023-2024");
    println!("         Aliyun revenue: FY2024 ~$15B+ (~12-15% of Alibaba Group revenue)");
    println!("         Aliyun growth: slow ~3-5% (post China regulatory tightening + AWS competition)");
    println!("         Alibaba Group market cap: $180-250B range");
    println!("  Strategic position: '#1 cloud in China + #4 globally + AI leader for Chinese market':");
    println!("                    pitch: 'China's most advanced cloud — AI, data, e-commerce platform infrastructure'");
    println!("                    target: Chinese enterprises + APAC region + global brands needing China presence");
    println!("                    primary competitor (China): Huawei Cloud, Tencent Cloud, Baidu Cloud");
    println!("                    primary competitor (global): AWS, Azure, GCP (Aliyun is #4)");
    println!("                    Aliyun's wedge: dominant in China + Singapore APAC hub + Qwen open-source LLM family");
    println!("                    China regulatory constraints limit international expansion potential");
    println!("                    cancelled IPO of Aliyun (Cloud Intelligence Group) Nov 2023");
    println!("  Pricing (transparent, sometimes aggressive in China + APAC):");
    println!("    ECS (compute): $0.005-$5/hr (varies massively by region + family)");
    println!("    OSS (object storage): $0.018/GB-month (standard) — comparable to S3");
    println!("    PolarDB: $0.06/hr (basic) up to $5+/hr (cluster)");
    println!("    PAI + Qwen API: Qwen-Max ~$0.012 per 1K input tokens (cheaper than GPT-4)");
    println!("    Qwen-Turbo: $0.0006 per 1K input tokens (highly competitive)");
    println!("    Free tier: 12 months free for new users (similar to AWS)");
    println!("    typically 10-30% cheaper than AWS in APAC + China");
    println!("  Product portfolio:");
    println!("    1. Elastic Compute Service (ECS):");
    println!("       - Equivalent to AWS EC2");
    println!("       - 100+ instance families (general, memory, GPU, FPGA, ARM-based)");
    println!("       - Yitian 710 ARM processor (Alibaba's own, T-Head/Pingtouge)");
    println!("       - 29+ regions globally, 80+ AZs");
    println!("    2. Object Storage Service (OSS):");
    println!("       - S3-compatible API");
    println!("       - Standard/IA/Archive tiers");
    println!("       - Powers Alibaba.com + Taobao + Tmall internally");
    println!("    3. PAI (Platform for AI) + Qwen family:");
    println!("       - PAI: ML/AI platform (training + inference + deployment)");
    println!("       - Qwen LLM family: Qwen-Max, Qwen-Plus, Qwen-Turbo, Qwen-VL (vision)");
    println!("       - Qwen-2.5 (2024): open-weights, competitive with Llama/Mistral");
    println!("       - Qwen-Coder, Qwen-Math, Qwen-Audio specialized variants");
    println!("       - DashScope API: Alibaba's LLM inference endpoint");
    println!("       - One of China's strongest open-source AI offerings");
    println!("    4. PolarDB (cloud-native distributed database):");
    println!("       - PostgreSQL + MySQL + Oracle-compatible variants");
    println!("       - Distributed, shared-storage architecture");
    println!("       - Compete with: AWS Aurora, Azure CosmosDB");
    println!("    5. MaxCompute (data warehouse):");
    println!("       - Petabyte-scale analytics (BigQuery-like)");
    println!("       - SQL + MapReduce + Graph + ML");
    println!("       - Powers Alibaba's internal e-commerce analytics");
    println!("    6. Container Service for Kubernetes (ACK):");
    println!("       - Managed K8s");
    println!("       - Serverless K8s (ASK)");
    println!("       - Strong in China for cloud-native deployment");
    println!("    7. CDN + DCDN (Dynamic Route for CDN):");
    println!("       - 3,200+ POPs globally");
    println!("       - Strong in APAC + China (Great Firewall navigation)");
    println!("       - Anti-DDoS Pro (large mitigation capacity)");
    println!("    8. Function Compute:");
    println!("       - Serverless functions (Lambda equivalent)");
    println!("       - Node/Python/Java/Go/PHP/Custom runtimes");
    println!("    9. ApsaraDB family:");
    println!("       - RDS for MySQL/PostgreSQL/SQL Server");
    println!("       - Redis, MongoDB, HBase, Cassandra");
    println!("    10. Pingtouge / T-Head silicon:");
    println!("       - Yitian 710 ARM server CPU (5nm)");
    println!("       - Hanguang 800 AI inference chip");
    println!("       - RISC-V XuanTie processors");
    println!("       - Strategic: domestic chip alternatives to AMD/Intel (geopolitical)");
    println!("  Apsara OS (the underlying platform):");
    println!("    - Aliyun's proprietary distributed OS — like Amazon's internal infrastructure");
    println!("    - 'Apsara' = sky deity in Sanskrit/Buddhist mythology");
    println!("    - Wang Jian's pet project — started 2008, criticized internally then vindicated");
    println!("    - Now powers all of Alibaba + Aliyun + Ant Group infrastructure");
    println!("    - Scales to 10,000+ nodes per cluster");
    println!("  Qwen LLM family (the AI bet):");
    println!("    - Open-weights release (Qwen, Qwen-1.5, Qwen-2, Qwen-2.5)");
    println!("    - Qwen-Max: closed-weights flagship (~GPT-4 level for Chinese)");
    println!("    - Qwen-VL: vision-language model");
    println!("    - Qwen-Audio: audio understanding model");
    println!("    - Strongest Chinese LLM family + globally competitive open-weights");
    println!("    - 100K+ downloads/month on HuggingFace");
    println!("    - Powers Tongyi Qianwen (Alibaba's ChatGPT competitor)");
    println!("  Geopolitical context:");
    println!("    - US export controls limit H100/A100 access in China");
    println!("    - Aliyun developing domestic chip alternatives (Hanguang, Yitian)");
    println!("    - 'Common prosperity' regulatory campaign 2020-2023 hurt Alibaba broadly");
    println!("    - Ant Group IPO blocked Nov 2020 — defining political moment");
    println!("    - Aliyun IPO cancelled Nov 2023");
    println!("    - China regulatory environment: continually adapting to political tides");
    println!("    - International expansion: limited by trust + sovereignty concerns");
    println!("  Integrations:");
    println!("    - Aliyun CLI (aliyun CLI, Go-based)");
    println!("    - Terraform + Pulumi providers");
    println!("    - Open API (REST) with SDK in 10+ languages");
    println!("    - Integration with Alibaba ecosystem: DingTalk, Taobao, Tmall");
    println!("    - HuggingFace + ModelScope (Alibaba's open-source AI hub)");
    println!("    - SDKs: Java, Python, Go, Node, .NET, PHP, Ruby");
    println!("  Aliyun CLI usage:");
    println!("    aliyun configure                                         # configure AccessKey");
    println!("    aliyun ecs DescribeInstances --RegionId=cn-hangzhou");
    println!("    aliyun ecs RunInstances --ImageId=ubuntu_22_04_x64_20G_alibase_20240826.vhd --InstanceType=ecs.t6-c1m1.large --RegionId=cn-hangzhou");
    println!("    aliyun oss mb oss://my-bucket --region=oss-cn-hangzhou");
    println!("    aliyun oss cp file.txt oss://my-bucket/");
    println!("    aliyun cs DescribeClusters                               # list ACK K8s clusters");
    println!("    aliyun pai dlc create-job --type=PyTorch --image=registry.ai/torch:2.0-gpu");
    println!("    aliyun ram CreateUser --UserName=my-user");
    println!("    aliyun dashscope completion --model=qwen-max --prompt='Hello'");
    println!("    aliyun cdn DescribeCdnDomain --DomainName=example.com");
    println!("  Customers (China + APAC + global):");
    println!("    - Alibaba ecosystem: Taobao, Tmall, Alipay (Ant), Cainiao, Lazada");
    println!("    - China enterprises: Geely, Xiaomi, BYD, banks, retailers");
    println!("    - APAC: Lazada, Tokopedia, Trip.com");
    println!("    - Global brands needing China presence: Philips, McDonald's, Bayer");
    println!("    - Government: many Chinese local + national government cloud workloads");
    println!("    - 4M+ paying customers");
    println!("  Critique: China regulatory environment chilled investor enthusiasm");
    println!("           Aliyun IPO cancelled Nov 2023 = strategic ambiguity");
    println!("           growth slowed dramatically (~3-5% in recent years)");
    println!("           international expansion limited by geopolitical trust concerns");
    println!("           US export controls limit GPU access for AI competitiveness");
    println!("           Huawei Cloud + Tencent Cloud + Baidu Cloud aggressive China competitors");
    println!("           public messaging shifted between aggressive expansion + retrenchment");
    println!("           Jack Ma absence post-2020 affects brand internationally");
    println!("           data sovereignty concerns for Western customers");
    println!("           Apsara internal-OS not portable / not multi-cloud");
    println!("  Differentiator: China's #1 cloud (Aliyun) + ~$15B revenue + global #4 by revenue + 29+ regions + Apsara internal OS (powers all of Alibaba) + Qwen LLM family (open-weights, Qwen-2.5 competitive with Llama/Mistral) + Tongyi Qianwen Chinese ChatGPT + T-Head/Pingtouge silicon (Yitian 710 ARM, Hanguang 800 AI chip — geopolitical resilience) + PolarDB distributed database + MaxCompute petabyte data warehouse + Alibaba e-commerce empire backbone (Taobao/Tmall/Alipay) + 4M+ paying customers + APAC dominance + dual NYSE:BABA / HKEX:9988 listing — the cloud platform that powers most of the Chinese internet and is China's open-source AI leader");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "alibabacloud".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_aliyun(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_aliyun};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/alibabacloud"), "alibabacloud");
        assert_eq!(basename(r"C:\bin\alibabacloud.exe"), "alibabacloud.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("alibabacloud.exe"), "alibabacloud");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_aliyun(&["--help".to_string()], "alibabacloud"), 0);
        assert_eq!(run_aliyun(&["-h".to_string()], "alibabacloud"), 0);
        assert_eq!(run_aliyun(&["--version".to_string()], "alibabacloud"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_aliyun(&[], "alibabacloud"), 0);
    }
}
